use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::Result;
use portable_pty::{ChildKiller, CommandBuilder, PtyPair, PtySize, PtySystem};
use vt100::Cell;
use vt100::Parser;

/// Lines of terminal history kept per session.
const SCROLLBACK_LINES: usize = 1000;

pub struct TerminalManager {
    pty_system: Box<dyn PtySystem + Send>,
    pty_pair: Option<PtyPair>,
    child_process: Arc<Mutex<Option<Box<dyn portable_pty::Child + Send>>>>,
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
    /// Bumped per shell start; threads of an older shell see a mismatch and
    /// leave the shared state to the current one.
    generation: Arc<AtomicU64>,
    writer: Arc<Mutex<Option<Box<dyn Write + Send>>>>,
    current_dir: Arc<Mutex<String>>,
    parser: Arc<Mutex<Parser>>,
    is_alive: Arc<Mutex<bool>>,
    disconnected: Arc<Mutex<bool>>,
    /// Bumped whenever the screen contents change, so the UI can skip redraws.
    output_version: Arc<AtomicU64>,
}

impl TerminalManager {
    pub fn new() -> Result<Self> {
        Ok(Self {
            pty_system: portable_pty::native_pty_system(),
            pty_pair: None,
            child_process: Arc::new(Mutex::new(None)),
            killer: None,
            generation: Arc::new(AtomicU64::new(0)),
            writer: Arc::new(Mutex::new(None)),
            current_dir: Arc::new(Mutex::new(
                std::env::current_dir()?.to_string_lossy().to_string(),
            )),
            parser: Arc::new(Mutex::new(Parser::new(24, 80, SCROLLBACK_LINES))),
            is_alive: Arc::new(Mutex::new(false)),
            disconnected: Arc::new(Mutex::new(false)),
            output_version: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn output_version(&self) -> u64 {
        self.output_version.load(Ordering::Relaxed)
    }

    pub fn is_disconnected(&self) -> bool {
        *self.disconnected.lock().unwrap()
    }

    pub fn restart(&mut self) {
        self.kill_shell();
        *self.disconnected.lock().unwrap() = false;
        *self.is_alive.lock().unwrap() = false;
        *self.writer.lock().unwrap() = None;
        *self.child_process.lock().unwrap() = None;
        self.pty_pair = None;
    }

    /// The reader thread holds a dup of the master fd, so dropping the pty
    /// alone never hangs up the shell; it has to be killed.
    fn kill_shell(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        if let Some(mut killer) = self.killer.take() {
            let _ = killer.kill();
        }
    }

    pub fn change_directory(&mut self, dir: &str) {
        *self.current_dir.lock().unwrap() = dir.to_string();
        // IMPORTANT: We intentionally do NOT send `cd ...` into a running shell.
        // Users want the embedded terminal session to remain stable and not to
        // echo `cd` lines into the UI. This path only affects the next PTY start.
    }

    pub fn send_input(&mut self, input: &str) {
        self.set_scroll(0);
        let mut guard = self.writer.lock().unwrap();
        if let Some(writer) = guard.as_mut() {
            let _ = writer.write_all(input.as_bytes());
            let _ = writer.flush();
        }
    }

    /// Current scrollback offset. vt100 clamps this to the history it actually
    /// holds, so it is the single source of truth rather than a mirrored field.
    fn scroll_offset(&self) -> usize {
        self.parser.lock().unwrap().screen().scrollback()
    }

    fn set_scroll(&self, rows: usize) {
        self.parser.lock().unwrap().set_scrollback(rows);
        self.output_version.fetch_add(1, Ordering::Relaxed);
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.set_scroll(self.scroll_offset().saturating_add(lines));
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.set_scroll(self.scroll_offset().saturating_sub(lines));
    }

    pub fn application_cursor(&self) -> bool {
        self.parser.lock().unwrap().screen().application_cursor()
    }

    pub fn is_scrolled_back(&self) -> bool {
        self.scroll_offset() > 0
    }

    pub fn update(&mut self) -> Result<()> {
        // Start terminal if not alive
        if !*self.is_alive.lock().unwrap() {
            // If the user exited the shell, keep the terminal disconnected until
            // explicitly restarted.
            if *self.disconnected.lock().unwrap() {
                return Ok(());
            }
            // Clean up any stale handles from a previously exited shell.
            // (Best-effort; most OS resources are freed when the child exits.)
            *self.writer.lock().unwrap() = None;
            *self.child_process.lock().unwrap() = None;
            self.start_terminal()?;
        }
        Ok(())
    }

    fn start_terminal(&mut self) -> Result<()> {
        let pty_pair = self.pty_system.openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        self.parser.lock().unwrap().set_size(24, 80);

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.cwd(&*self.current_dir.lock().unwrap());
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        // Set up for interactive shell (best-effort)
        let shell_name = Path::new(&shell)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if shell_name == "bash" || shell_name == "zsh" || shell_name == "fish" {
            cmd.arg("-i");
        }

        let child = pty_pair.slave.spawn_command(cmd)?;
        self.killer = Some(child.clone_killer());
        let gen = self.generation.load(Ordering::SeqCst);

        // Store child so we can manage lifecycle (and allow a watcher thread to wait).
        *self.child_process.lock().unwrap() = Some(child);

        *self.is_alive.lock().unwrap() = true;
        *self.disconnected.lock().unwrap() = false;

        // Watcher thread: if the shell exits (e.g. user types `exit`), mark
        // the session as dead so update() can restart it.
        {
            let child_process = self.child_process.clone();
            let is_alive = self.is_alive.clone();
            let writer = self.writer.clone();
            let disconnected = self.disconnected.clone();
            let parser = self.parser.clone();
            let version = self.output_version.clone();
            let generation = self.generation.clone();
            thread::spawn(move || {
                let child = child_process.lock().unwrap().take();
                if let Some(mut child) = child {
                    let _ = child.wait();
                }
                if generation.load(Ordering::SeqCst) != gen {
                    return;
                }
                *is_alive.lock().unwrap() = false;
                *disconnected.lock().unwrap() = true;
                *writer.lock().unwrap() = None;

                // Best-effort message into the terminal buffer so the user knows
                // what happened and how to recover.
                if let Ok(mut p) = parser.lock() {
                    p.process(b"\r\n[Shell exited. Press R in the list to restart]\r\n");
                }
                version.fetch_add(1, Ordering::Relaxed);
            });
        }

        // Take the writer ONCE and store it.
        *self.writer.lock().unwrap() = Some(pty_pair.master.take_writer()?);

        let mut reader = pty_pair.master.try_clone_reader()?;
        let parser = self.parser.clone();
        let version = self.output_version.clone();
        let generation = self.generation.clone();

        // Reader thread to capture output. vt100 is a full terminal parser, so
        // escape sequences (OSC included) are consumed here, not hand-stripped.
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if generation.load(Ordering::SeqCst) != gen => break,
                    Ok(n) => {
                        parser.lock().unwrap().process(&buf[..n]);
                        version.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        });

        self.pty_pair = Some(pty_pair);

        Ok(())
    }

    pub fn get_screen_cells(&self, rows: u16, cols: u16) -> Vec<Vec<Cell>> {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let mut parser = self.parser.lock().unwrap();
        parser.set_size(rows, cols);

        // set_scrollback has already shifted the view, so the visible screen is
        // either the live one or the scrolled-back window.
        let screen = parser.screen();
        (0..rows)
            .map(|r| {
                (0..cols)
                    .map(|c| screen.cell(r, c).cloned().unwrap_or_default())
                    .collect()
            })
            .collect()
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        if let Some(ref pty_pair) = self.pty_pair {
            let _ = pty_pair.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        self.parser.lock().unwrap().set_size(rows, cols);
    }
}

impl Drop for TerminalManager {
    fn drop(&mut self) {
        self.kill_shell();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn child_shells() -> usize {
        let out = std::process::Command::new("pgrep")
            .args(["-P", &std::process::id().to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).lines().count()
    }

    #[test]
    fn restart_and_drop_leave_no_stray_shells() {
        let mut tm = TerminalManager::new().unwrap();
        tm.update().unwrap();
        tm.restart();
        tm.update().unwrap();
        thread::sleep(Duration::from_millis(500));
        // The killed shell's watcher must not disconnect its replacement.
        assert!(!tm.is_disconnected());
        assert!(tm.writer.lock().unwrap().is_some());
        assert_eq!(child_shells(), 1);

        drop(tm);
        thread::sleep(Duration::from_millis(500));
        assert_eq!(child_shells(), 0);
    }
}
