use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use anyhow::Result;
use portable_pty::{CommandBuilder, PtyPair, PtySize, PtySystem};
use vt100::Parser;
use vt100::Cell;

pub struct TerminalManager {
    pty_system: Box<dyn PtySystem + Send>,
    pty_pair: Option<PtyPair>,
    child_process: Arc<Mutex<Option<Box<dyn portable_pty::Child + Send>>>>,
    writer: Arc<Mutex<Option<Box<dyn Write + Send>>>>,
    current_dir: Arc<Mutex<String>>,
    parser: Arc<Mutex<Parser>>,
    is_alive: Arc<Mutex<bool>>,
    disconnected: Arc<Mutex<bool>>,
    /// Raw output bytes accumulated from PTY (for scrollback re-rendering)
    raw_output: Arc<Mutex<Vec<u8>>>,
    /// Scroll offset (0 = live view, >0 = scrolled back)
    scroll_offset: usize,
}

impl TerminalManager {
    pub fn new() -> Result<Self> {
        let pty_system = portable_pty::native_pty_system();
        let parser = Parser::new(24, 80, 1000);

        Ok(Self {
            pty_system,
            pty_pair: None,
            child_process: Arc::new(Mutex::new(None)),
            writer: Arc::new(Mutex::new(None)),
            current_dir: Arc::new(Mutex::new(std::env::current_dir()?.to_string_lossy().to_string())),
            parser: Arc::new(Mutex::new(parser)),
            is_alive: Arc::new(Mutex::new(false)),
            disconnected: Arc::new(Mutex::new(false)),
            raw_output: Arc::new(Mutex::new(Vec::new())),
            scroll_offset: 0,
        })
    }

    pub fn is_disconnected(&self) -> bool {
        *self.disconnected.lock().unwrap()
    }

    pub fn restart(&mut self) {
        *self.disconnected.lock().unwrap() = false;
        *self.is_alive.lock().unwrap() = false;
        *self.writer.lock().unwrap() = None;
        *self.child_process.lock().unwrap() = None;
        self.pty_pair = None;
    }

    pub fn change_directory(&mut self, dir: &str) {
        *self.current_dir.lock().unwrap() = dir.to_string();
        // IMPORTANT: We intentionally do NOT send `cd ...` into a running shell.
        // Users want the embedded terminal session to remain stable and not to
        // echo `cd` lines into the UI. This path only affects the next PTY start.
    }

    pub fn send_input(&mut self, input: &str) {
        self.scroll_offset = 0;
        let mut guard = self.writer.lock().unwrap();
        if let Some(writer) = guard.as_mut() {
            let _ = writer.write_all(input.as_bytes());
            let _ = writer.flush();
        }
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset += lines;
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    pub fn is_scrolled_back(&self) -> bool {
        self.scroll_offset > 0
    }

    pub async fn update(&mut self) -> Result<()> {
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

        {
            let mut parser = self.parser.lock().unwrap();
            parser.set_size(24, 80);
        }

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

        // Store child so we can manage lifecycle (and allow a watcher thread to wait).
        {
            let mut guard = self.child_process.lock().unwrap();
            *guard = Some(child);
        }

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
            thread::spawn(move || {
                let child = {
                    let mut guard = child_process.lock().unwrap();
                    guard.take()
                };
                if let Some(mut child) = child {
                    let _ = child.wait();
                }
                *is_alive.lock().unwrap() = false;
                *disconnected.lock().unwrap() = true;
                *writer.lock().unwrap() = None;

                // Best-effort message into the terminal buffer so the user knows
                // what happened and how to recover.
                if let Ok(mut p) = parser.lock() {
                    p.process(b"\r\n[Shell exited. Press Ctrl+R (or Shift+R in list) to restart]\r\n");
                }
            });
        }

        // Take the writer ONCE and store it.
        {
            let mut guard = self.writer.lock().unwrap();
            *guard = Some(pty_pair.master.take_writer()?);
        }

        let mut reader = pty_pair.master.try_clone_reader()?;
        let parser = self.parser.clone();
        let raw_output = self.raw_output.clone();

        // Reader thread to capture output
        thread::spawn(move || {
            #[derive(Debug, Clone, Copy)]
            enum ParseState {
                Normal,
                Esc,
                Osc,
                OscEsc,
            }

            // Strip OSC sequences (ESC ] ... BEL) or (ESC ] ... ESC \\)
            // These are often emitted by shell integration (e.g. iTerm2) and
            // show up as noisy text in a non-ANSI-aware renderer.
            fn strip_osc(bytes: &[u8], state: &mut ParseState) -> Vec<u8> {
                let mut out = Vec::with_capacity(bytes.len());
                for &b in bytes {
                    match *state {
                        ParseState::Normal => {
                            if b == 0x1b {
                                *state = ParseState::Esc;
                            } else {
                                out.push(b);
                            }
                        }
                        ParseState::Esc => {
                            if b == b']' {
                                *state = ParseState::Osc;
                            } else {
                                // Not an OSC sequence; keep ESC + this byte.
                                out.push(0x1b);
                                out.push(b);
                                *state = ParseState::Normal;
                            }
                        }
                        ParseState::Osc => {
                            if b == 0x07 {
                                // BEL terminator
                                *state = ParseState::Normal;
                            } else if b == 0x1b {
                                // Might be ESC \\ terminator
                                *state = ParseState::OscEsc;
                            }
                        }
                        ParseState::OscEsc => {
                            if b == b'\\' {
                                // ESC \\ terminator
                                *state = ParseState::Normal;
                            } else {
                                // Still inside OSC; keep consuming.
                                *state = ParseState::Osc;
                            }
                        }
                    }
                }
                out
            }

            let mut state = ParseState::Normal;
            let mut buf = [0u8; 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // EOF
                    Ok(n) => {
                        let cleaned = strip_osc(&buf[..n], &mut state);
                        if cleaned.is_empty() {
                            continue;
                        }
                        raw_output.lock().unwrap().extend_from_slice(&cleaned);
                        let mut parser = parser.lock().unwrap();
                        parser.process(&cleaned);
                    }
                    Err(_) => break,
                }
            }
        });

        self.pty_pair = Some(pty_pair);

        Ok(())
    }

    pub fn get_screen_lines(&self, rows: u16, cols: u16) -> Vec<String> {
        let mut parser = self.parser.lock().unwrap();
        parser.set_size(rows, cols);

        let text = parser.screen().contents();
        let mut lines: Vec<String> = text
            .lines()
            .map(|l| {
                let mut s = l.to_string();
                if s.chars().count() > cols as usize {
                    s = s.chars().take(cols as usize).collect();
                }
                s
            })
            .collect();

        if lines.len() > rows as usize {
            lines = lines[lines.len().saturating_sub(rows as usize)..].to_vec();
        }

        while lines.len() < rows as usize {
            lines.insert(0, String::new());
        }

        lines
    }

    pub fn get_screen_cells(&self, rows: u16, cols: u16) -> Vec<Vec<Cell>> {
        let mut parser = self.parser.lock().unwrap();
        parser.set_size(rows, cols);

        if self.scroll_offset == 0 {
            // Live view — just return visible screen
            let screen = parser.screen();
            let mut out: Vec<Vec<Cell>> = Vec::with_capacity(rows as usize);
            for r in 0..rows {
                let mut row: Vec<Cell> = Vec::with_capacity(cols as usize);
                for c in 0..cols {
                    let cell = screen.cell(r, c).cloned().unwrap_or_default();
                    row.push(cell);
                }
                out.push(row);
            }
            return out;
        }

        // Scrollback view — re-render all raw output into a tall virtual terminal,
        // then pick a window offset from the bottom.
        let raw = self.raw_output.lock().unwrap();
        if raw.is_empty() {
            // Nothing to scroll back to
            let screen = parser.screen();
            let mut out: Vec<Vec<Cell>> = Vec::with_capacity(rows as usize);
            for r in 0..rows {
                let mut row: Vec<Cell> = Vec::with_capacity(cols as usize);
                for c in 0..cols {
                    let cell = screen.cell(r, c).cloned().unwrap_or_default();
                    row.push(cell);
                }
                out.push(row);
            }
            return out;
        }

        // Use a large virtual terminal to hold all output
        let tall_rows = 5000u16; // max scrollback lines
        let mut tmp = Parser::new(tall_rows, cols, 0);
        tmp.process(&raw);

        let tmp_screen = tmp.screen();

        // Find the last non-empty row to know the content height
        let mut last_row = 0usize;
        for r in (0..tall_rows).rev() {
            let mut has_content = false;
            for c in 0..cols {
                if let Some(cell) = tmp_screen.cell(r, c) {
                    if cell.has_contents() {
                        has_content = true;
                        break;
                    }
                }
            }
            if has_content {
                last_row = r as usize + 1;
                break;
            }
        }

        let content_height = last_row.max(rows as usize);
        let max_offset = content_height.saturating_sub(rows as usize);
        let offset = self.scroll_offset.min(max_offset);
        let start = content_height.saturating_sub(rows as usize + offset);

        let mut out: Vec<Vec<Cell>> = Vec::with_capacity(rows as usize);
        for r in 0..rows as usize {
            let actual_row = (start + r) as u16;
            let mut row: Vec<Cell> = Vec::with_capacity(cols as usize);
            for c in 0..cols {
                let cell = if actual_row < tall_rows {
                    tmp_screen.cell(actual_row, c).cloned().unwrap_or_default()
                } else {
                    Cell::default()
                };
                row.push(cell);
            }
            out.push(row);
        }
        out
    }

    pub fn is_alive(&self) -> bool {
        *self.is_alive.lock().unwrap()
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        if let Some(ref pty_pair) = self.pty_pair {
            let _ = pty_pair.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }

        let mut parser = self.parser.lock().unwrap();
        parser.set_size(rows, cols);
    }
}
