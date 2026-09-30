use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers, MouseEventKind};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::widgets::ListState;
use ratatui::{backend::Backend, Terminal};

use wt_core::git::GitRepo;
use wt_core::types::{AddOptions, WorktreeInfo};
use wt_core::worktree;
use crate::terminal::TerminalManager;
use crate::ui::draw;

pub struct App {
    pub repo: GitRepo,
    pub worktrees: Vec<WorktreeInfo>,
    pub selected_index: usize,
    /// Owned by App so the list's scroll offset persists between frames.
    pub list_state: ListState,
    pub terminal_manager: TerminalManager,
    pub terminal_sessions: HashMap<String, TerminalManager>,
    pub active_terminal_path: String,
    pub focus: Focus,
    pub base_path: String,
    /// Branch candidates for add-modal completion, loaded when the modal opens.
    branches: Vec<String>,
    pub should_quit: bool,
    add_modal_state: AddWorktreeModal,
    progress_overlay: Option<String>,
    error_message: Option<String>,
    pending_action: Option<PendingAction>,
    confirm_dialog: Option<ConfirmDialog>,
    _watcher: Option<RecommendedWatcher>,
    git_changed_rx: mpsc::Receiver<()>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Terminal,
}

#[derive(Debug, Default)]
pub struct AddWorktreeModal {
    pub visible: bool,
    pub input: String,
    pub error: Option<String>,
    pub is_submitting: bool,
}

#[derive(Debug)]
enum PendingAction {
    AddWorktree { branch: String },
    RemoveWorktree { path: String },
    PruneWorktrees,
}

#[derive(Debug)]
pub(crate) enum ConfirmAction {
    RemoveWorktree { path: String },
    PruneWorktrees,
}

#[derive(Debug)]
pub struct ConfirmDialog {
    pub message: String,
    pub(crate) action: ConfirmAction,
}

impl App {
    pub fn new(repo_path: &str) -> Result<Self> {
        let repo = GitRepo::open(std::path::Path::new(repo_path))?;
        let worktrees = worktree::list_worktrees(&repo)?;
        let base_path = repo.repo_root()?.to_string_lossy().to_string();

        // Find base worktree index
        let selected_index = worktrees
            .iter()
            .position(|wt| wt.path == base_path)
            .unwrap_or(0);

        let terminal_manager = TerminalManager::new()?;

        let active_terminal_path = worktrees
            .get(selected_index)
            .map(|wt| wt.path.clone())
            .unwrap_or_else(|| base_path.clone());

        // Set up file watcher for git state changes
        let (tx, rx) = mpsc::channel();
        let git_dir = PathBuf::from(&base_path).join(".git");

        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                let dominated = ev.paths.iter().any(|p| {
                    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    name == "HEAD" || name == "FETCH_HEAD" || p.to_string_lossy().contains("refs")
                });
                if dominated {
                    let _ = tx.send(());
                }
            }
        });

        let watcher = if let Ok(mut w) = watcher {
            let _ = w.watch(&git_dir, RecursiveMode::Recursive);
            Some(w)
        } else {
            None
        };

        Ok(Self {
            repo,
            worktrees,
            selected_index,
            list_state: ListState::default(),
            terminal_manager,
            terminal_sessions: HashMap::new(),
            active_terminal_path,
            focus: Focus::List,
            base_path,
            branches: Vec::new(),
            should_quit: false,
            add_modal_state: AddWorktreeModal::default(),
            progress_overlay: None,
            error_message: None,
            pending_action: None,
            confirm_dialog: None,
            _watcher: watcher,
            git_changed_rx: rx,
        })
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        // Redraw only when something actually changed: an input event, new PTY
        // output, or a state transition. Otherwise this loop repaints 60x/sec
        // at idle for no reason.
        let mut needs_redraw = true;
        let mut last_output_version = 0u64;

        loop {
            let output_version = self.terminal_manager.output_version();
            if needs_redraw || output_version != last_output_version {
                terminal.draw(|f| draw::<B>(f, self))?;
                last_output_version = output_version;
                needs_redraw = false;
            }

            // If the shell exited while focused on the terminal, switch focus back
            // to the list to avoid a "dead" terminal pane capturing input.
            if self.focus == Focus::Terminal && self.terminal_manager.is_disconnected() {
                self.focus = Focus::List;
                needs_redraw = true;
            }

            if self.process_pending_action() {
                needs_redraw = true;
            }

            // Refresh worktree list when git state changes
            if self.git_changed_rx.try_recv().is_ok() {
                // Drain any extra pending signals
                while self.git_changed_rx.try_recv().is_ok() {}
                self.refresh_worktrees();
                needs_redraw = true;
            }

            // Handle events
            if event::poll(Duration::from_millis(16))? {
                // Any event can change the view; the terminal may also have been
                // swapped out from under last_output_version by a selection change.
                needs_redraw = true;
                last_output_version = 0;
                match event::read()? {
                    Event::Key(key) => {
                        // Any keypress in terminal scrollback → snap back to live
                        if self.focus == Focus::Terminal && self.terminal_manager.is_scrolled_back() {
                            self.terminal_manager.scroll_down(usize::MAX);
                        }
                        self.handle_key(key.code, key.modifiers);
                    }
                    Event::Paste(text) => self.paste(&text),
                    Event::Mouse(mouse) => match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            if self.focus == Focus::Terminal {
                                self.terminal_manager.scroll_up(3);
                            } else if self.selected_index > 0 {
                                self.selected_index -= 1;
                                self.update_terminal_for_selection();
                            }
                        }
                        MouseEventKind::ScrollDown => {
                            if self.focus == Focus::Terminal {
                                self.terminal_manager.scroll_down(3);
                            } else if self.selected_index < self.worktrees.len().saturating_sub(1) {
                                self.selected_index += 1;
                                self.update_terminal_for_selection();
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }

            // Always keep terminal alive (even when list is focused)
            self.terminal_manager.update()?;

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn handle_key(&mut self, key_code: KeyCode, modifiers: KeyModifiers) {
        if self.error_message.is_some() {
            self.clear_error();
            return;
        }

        if self.confirm_dialog.is_some() {
            self.handle_confirm_key(key_code);
            return;
        }

        if self.progress_overlay.is_some() {
            // Ignore input while a blocking operation is in progress.
            return;
        }

        if self.add_modal_state.visible {
            self.handle_add_modal_key(key_code, modifiers);
            return;
        }
        match self.focus {
            Focus::List => self.handle_list_key(key_code, modifiers),
            Focus::Terminal => self.handle_terminal_key(key_code, modifiers),
        }
    }

    fn handle_list_key(&mut self, key_code: KeyCode, _modifiers: KeyModifiers) {
        match key_code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('g') => self.refresh_worktrees(),
            KeyCode::Char('r') => self.confirm_remove_selected(),
            KeyCode::Char('R') => {
                self.terminal_manager.restart();
                self.focus = Focus::Terminal;
            }
            KeyCode::Char('a') => self.open_add_modal(),
            KeyCode::Char('x') => self.confirm_prune_worktrees(),
            KeyCode::Up => {
                if self.selected_index > 0 {
                    self.selected_index -= 1;
                    self.update_terminal_for_selection();
                }
            }
            KeyCode::Down => {
                if self.selected_index < self.worktrees.len().saturating_sub(1) {
                    self.selected_index += 1;
                    self.update_terminal_for_selection();
                }
            }
            KeyCode::Enter => {
                self.focus = Focus::Terminal;
                self.update_terminal_for_selection();
            }
            KeyCode::Tab => self.focus = Focus::Terminal,
            _ => {}
        }
    }

    /// Ctrl+T is the only key the pane keeps; everything else — Esc for vim,
    /// Ctrl+R for history search — belongs to the shell.
    fn handle_terminal_key(&mut self, key_code: KeyCode, modifiers: KeyModifiers) {
        if key_code == KeyCode::Char('t') && modifiers.contains(KeyModifiers::CONTROL) {
            self.focus = Focus::List;
            return;
        }
        let app_cursor = self.terminal_manager.application_cursor();
        if let Some(input) = key_to_ansi(key_code, modifiers, app_cursor) {
            self.terminal_manager.send_input(&input);
        }
    }

    /// Bracketed paste: into the add-modal field if it is open, otherwise
    /// straight through to the shell.
    pub fn paste(&mut self, text: &str) {
        if self.add_modal_state.visible {
            if self.add_modal_state.is_submitting {
                return;
            }
            // A branch name is one line; newlines would submit unpredictably.
            for line in text.lines() {
                self.add_modal_state.input.push_str(line);
            }
            self.add_modal_state.error = None;
        } else if self.focus == Focus::Terminal {
            self.terminal_manager.send_input(text);
        }
    }

    fn refresh_worktrees(&mut self) {
        if let Ok(wts) = worktree::list_worktrees(&self.repo) {
            self.worktrees = wts;
            if self.selected_index >= self.worktrees.len() {
                self.selected_index = self.worktrees.len().saturating_sub(1);
            }
            // Drop shells belonging to worktrees that no longer exist, otherwise
            // every worktree ever visited keeps a live process until quit.
            let live: HashSet<&str> = self.worktrees.iter().map(|wt| wt.path.as_str()).collect();
            self.terminal_sessions
                .retain(|path, _| live.contains(path.as_str()));
        }
    }

    fn confirm_remove_selected(&mut self) {
        if let Some(wt) = self.worktrees.get(self.selected_index) {
            self.confirm_dialog = Some(ConfirmDialog {
                message: format!("Remove worktree \"{}\"?", wt.path),
                action: ConfirmAction::RemoveWorktree {
                    path: wt.path.clone(),
                },
            });
        }
    }

    fn confirm_prune_worktrees(&mut self) {
        self.confirm_dialog = Some(ConfirmDialog {
            message: "Prune reachable worktrees?".to_string(),
            action: ConfirmAction::PruneWorktrees,
        });
    }

    fn update_terminal_for_selection(&mut self) {
        let path = self
            .worktrees
            .get(self.selected_index)
            .map(|wt| wt.path.clone());

        if let Some(path) = path {
            self.switch_terminal_session(&path);
        }
    }

    fn switch_terminal_session(&mut self, target_path: &str) {
        if self.active_terminal_path == target_path {
            // Still update desired cwd for the (not-yet-started) session.
            self.terminal_manager.change_directory(target_path);
            return;
        }

        let placeholder = match TerminalManager::new() {
            Ok(tm) => tm,
            Err(_) => return,
        };

        let current_path = std::mem::take(&mut self.active_terminal_path);
        let current_manager = std::mem::replace(&mut self.terminal_manager, placeholder);
        self.terminal_sessions.insert(current_path, current_manager);

        let mut next = if let Some(existing) = self.terminal_sessions.remove(target_path) {
            existing
        } else {
            match TerminalManager::new() {
                Ok(tm) => tm,
                Err(_) => return,
            }
        };
        next.change_directory(target_path);

        self.terminal_manager = next;
        self.active_terminal_path = target_path.to_string();
    }

    fn open_add_modal(&mut self) {
        self.branches = self.repo.list_branch_candidates().unwrap_or_default();
        self.add_modal_state.visible = true;
        self.add_modal_state.input.clear();
        self.add_modal_state.error = None;
        self.add_modal_state.is_submitting = false;
    }

    fn close_add_modal(&mut self) {
        self.add_modal_state.visible = false;
        self.add_modal_state.error = None;
        self.add_modal_state.input.clear();
        self.add_modal_state.is_submitting = false;
    }

    fn handle_add_modal_key(&mut self, key_code: KeyCode, modifiers: KeyModifiers) {
        if self.add_modal_state.is_submitting {
            return;
        }
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        match key_code {
            KeyCode::Esc => self.close_add_modal(),
            KeyCode::Enter => self.submit_add_modal(),
            KeyCode::Backspace => {
                self.add_modal_state.input.pop();
            }
            // Complete to the longest prefix shared by every match, like a shell.
            KeyCode::Tab => {
                let matches = self.branch_matches();
                if !matches.is_empty() {
                    let completed = longest_common_prefix(&matches);
                    if completed.len() > self.add_modal_state.input.len() {
                        self.add_modal_state.input = completed;
                    }
                }
            }
            KeyCode::Char('u') if ctrl => self.add_modal_state.input.clear(),
            KeyCode::Char('w') if ctrl => delete_last_word(&mut self.add_modal_state.input),
            KeyCode::Char(c) => {
                if ctrl {
                    return;
                }
                self.add_modal_state.input.push(c);
            }
            _ => {}
        }
        self.add_modal_state.error = None;
    }

    fn submit_add_modal(&mut self) {
        let raw_input = self.add_modal_state.input.trim().to_string();
        if raw_input.is_empty() {
            self.add_modal_state.error = Some("Branch name is required".to_string());
            return;
        }

        self.add_modal_state.is_submitting = true;
        self.add_modal_state.error = None;
        if self.pending_action.is_none() {
            self.show_progress_overlay("Creating worktree...");
            self.pending_action = Some(PendingAction::AddWorktree {
                branch: raw_input,
            });
        }
    }

    /// Branch candidates matching what has been typed so far.
    pub fn branch_matches(&self) -> Vec<&str> {
        let input = self.add_modal_state.input.trim();
        if input.is_empty() {
            return Vec::new();
        }
        self.branches
            .iter()
            .filter(|b| b.starts_with(input) && b.as_str() != input)
            .map(|b| b.as_str())
            .collect()
    }

    pub fn add_modal(&self) -> &AddWorktreeModal {
        &self.add_modal_state
    }

    pub fn add_modal_visible(&self) -> bool {
        self.add_modal_state.visible
    }

    pub fn progress_overlay(&self) -> Option<&str> {
        self.progress_overlay.as_deref()
    }

    fn show_progress_overlay<S: Into<String>>(&mut self, message: S) {
        self.progress_overlay = Some(message.into());
    }

    fn hide_progress_overlay(&mut self) {
        self.progress_overlay = None;
    }

    pub fn error_message(&self) -> Option<&str> {
        self.error_message.as_deref()
    }

    fn set_error<S: Into<String>>(&mut self, msg: S) {
        self.error_message = Some(msg.into());
    }

    fn clear_error(&mut self) {
        self.error_message = None;
    }

    pub fn confirm_message(&self) -> Option<&str> {
        self.confirm_dialog.as_ref().map(|dialog| dialog.message.as_str())
    }

    fn handle_confirm_key(&mut self, key_code: KeyCode) {
        match key_code {
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(dialog) = self.confirm_dialog.take() {
                    self.execute_confirm_action(dialog.action);
                }
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                self.confirm_dialog = None;
            }
            _ => {}
        }
    }

    fn execute_confirm_action(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::RemoveWorktree { path } => {
                if self.pending_action.is_none() {
                    self.show_progress_overlay("Removing worktree...");
                    self.pending_action = Some(PendingAction::RemoveWorktree { path });
                }
            }
            ConfirmAction::PruneWorktrees => {
                if self.pending_action.is_none() {
                    self.show_progress_overlay("Pruning worktrees...");
                    self.pending_action = Some(PendingAction::PruneWorktrees);
                }
            }
        }
    }

    /// Returns true if an action ran (and therefore the UI needs a repaint).
    fn process_pending_action(&mut self) -> bool {
        let Some(action) = self.pending_action.take() else {
            return false;
        };

        match action {
            PendingAction::AddWorktree { branch } => {
                let opts = AddOptions {
                    branch: branch.clone(),
                    ..AddOptions::default()
                };
                match worktree::add_worktree(&self.repo, opts) {
                    Ok(_) => {
                        self.clear_error();
                        self.close_add_modal();
                        self.refresh_worktrees();
                        if let Some(idx) = self
                            .worktrees
                            .iter()
                            .position(|wt| wt.branch.as_deref() == Some(branch.as_str()))
                        {
                            self.selected_index = idx;
                        }
                        self.update_terminal_for_selection();
                    }
                    Err(err) => {
                        self.set_error(format!("Failed to create worktree: {}", err));
                        self.add_modal_state.error = Some(err.to_string());
                        self.add_modal_state.is_submitting = false;
                    }
                }
            }
            PendingAction::RemoveWorktree { path } => {
                let opts = wt_core::types::RemoveOptions {
                    target: path.clone(),
                    force: false,
                    as_branch: false,
                    as_path: true,
                };
                match worktree::remove_worktree(&self.repo, opts) {
                    Ok(_) => {
                        self.clear_error();
                        self.refresh_worktrees();
                    }
                    Err(err) => {
                        self.set_error(format!("Failed to remove worktree: {}", err));
                    }
                }
            },
            PendingAction::PruneWorktrees => {
                let opts = wt_core::types::PruneOptions {
                    dry_run: false,
                    verbose: false,
                    expire: None,
                };
                let _ = worktree::prune_worktrees(&self.repo, opts);
                self.refresh_worktrees();
            }
        }

        self.hide_progress_overlay();
        true
    }
}

/// Bytes an xterm sends for a key. `app_cursor` is DECCKM, which full-screen
/// programs like vim and less switch on to get `ESC O` arrows.
fn key_to_ansi(key_code: KeyCode, modifiers: KeyModifiers, app_cursor: bool) -> Option<String> {
    let arrow = |c: char| {
        if app_cursor { format!("\x1bO{}", c) } else { format!("\x1b[{}", c) }
    };
    let seq = match key_code {
        KeyCode::Enter => "\r".to_string(),
        KeyCode::Backspace => "\x7f".to_string(),
        KeyCode::Delete => "\x1b[3~".to_string(),
        KeyCode::Insert => "\x1b[2~".to_string(),
        KeyCode::Tab => "\t".to_string(),
        KeyCode::BackTab => "\x1b[Z".to_string(),
        KeyCode::Esc => "\x1b".to_string(),
        KeyCode::Up => arrow('A'),
        KeyCode::Down => arrow('B'),
        KeyCode::Right => arrow('C'),
        KeyCode::Left => arrow('D'),
        KeyCode::Home => arrow('H'),
        KeyCode::End => arrow('F'),
        KeyCode::PageUp => "\x1b[5~".to_string(),
        KeyCode::PageDown => "\x1b[6~".to_string(),
        KeyCode::F(n @ 1..=4) => format!("\x1bO{}", (b'P' + n - 1) as char),
        KeyCode::F(n @ 5..=12) => {
            const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            format!("\x1b[{}~", CODES[(n - 5) as usize])
        }
        KeyCode::Char(c) if modifiers.contains(KeyModifiers::CONTROL) => {
            let byte = match c.to_ascii_lowercase() {
                c @ 'a'..='z' => c as u8 - b'a' + 1,
                '@' | ' ' | '2' => 0,
                '[' | '3' => 0x1b,
                '\\' | '4' => 0x1c,
                ']' | '5' => 0x1d,
                '^' | '6' => 0x1e,
                '_' | '-' | '7' => 0x1f,
                '8' | '?' => 0x7f,
                _ => return None,
            };
            (byte as char).to_string()
        }
        KeyCode::Char(c) => c.to_string(),
        _ => return None,
    };
    // Alt sends ESC first (meta-sends-escape), which readline and zle read as Meta.
    if modifiers.contains(KeyModifiers::ALT) {
        Some(format!("\x1b{}", seq))
    } else {
        Some(seq)
    }
}

/// Longest prefix shared by every candidate.
fn longest_common_prefix(items: &[&str]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let mut end = first.len();
    for item in &items[1..] {
        end = end.min(
            first
                .char_indices()
                .zip(item.char_indices())
                .take_while(|((_, a), (_, b))| a == b)
                .last()
                .map_or(0, |((i, c), _)| i + c.len_utf8()),
        );
    }
    first[..end].to_string()
}

/// Delete back to the previous `/` or space — branch names are path-like.
fn delete_last_word(input: &mut String) {
    while input.ends_with('/') || input.ends_with(' ') {
        input.pop();
    }
    while !input.is_empty() && !input.ends_with('/') && !input.ends_with(' ') {
        input.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completes_to_the_shared_prefix() {
        assert_eq!(longest_common_prefix(&["feat/login", "feat/logout"]), "feat/log");
        assert_eq!(longest_common_prefix(&["only-one"]), "only-one");
        assert_eq!(longest_common_prefix(&["abc", "xyz"]), "");
        assert_eq!(longest_common_prefix(&[]), "");
        // must not split a multi-byte char
        assert_eq!(longest_common_prefix(&["功能-a", "功能-b"]), "功能-");
    }

    #[test]
    fn forwards_shell_keys_as_xterm_does() {
        let none = KeyModifiers::NONE;
        let ctrl = KeyModifiers::CONTROL;
        let key = |k, m| key_to_ansi(k, m, false);
        assert_eq!(key(KeyCode::Esc, none).as_deref(), Some("\x1b"));
        assert_eq!(key(KeyCode::Char('r'), ctrl).as_deref(), Some("\x12"));
        assert_eq!(key(KeyCode::Char('R'), ctrl).as_deref(), Some("\x12"));
        assert_eq!(key(KeyCode::Char('['), ctrl).as_deref(), Some("\x1b"));
        assert_eq!(key(KeyCode::Char('b'), KeyModifiers::ALT).as_deref(), Some("\x1bb"));
        assert_eq!(key(KeyCode::BackTab, KeyModifiers::SHIFT).as_deref(), Some("\x1b[Z"));
        assert_eq!(key(KeyCode::F(1), none).as_deref(), Some("\x1bOP"));
        assert_eq!(key(KeyCode::F(12), none).as_deref(), Some("\x1b[24~"));
        assert_eq!(key(KeyCode::Up, none).as_deref(), Some("\x1b[A"));
        assert_eq!(key_to_ansi(KeyCode::Up, none, true).as_deref(), Some("\x1bOA"));
    }

    #[test]
    fn deletes_one_path_segment() {
        // The separator is kept so the next segment can be typed straight away.
        let mut s = String::from("feature/login/form");
        delete_last_word(&mut s);
        assert_eq!(s, "feature/login/");
        delete_last_word(&mut s);
        assert_eq!(s, "feature/");
        delete_last_word(&mut s);
        assert_eq!(s, "");
        delete_last_word(&mut s);
        assert_eq!(s, "");
    }
}
