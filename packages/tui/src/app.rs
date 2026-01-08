use std::{collections::HashMap, time::Duration};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::{backend::Backend, Terminal};

use crate::{git::GitRepo, terminal::TerminalManager, ui::draw};

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub is_base: bool,
    pub is_locked: bool,
    pub is_prunable: bool,
}

pub struct App {
    pub repo: GitRepo,
    pub worktrees: Vec<Worktree>,
    pub selected_index: usize,
    pub terminal_manager: TerminalManager,
    pub terminal_sessions: HashMap<String, TerminalManager>,
    pub active_terminal_path: String,
    pub focus: Focus,
    pub base_path: String,
    pub should_quit: bool,
    add_modal_state: AddWorktreeModal,
    progress_overlay: Option<String>,
    error_message: Option<String>,
    pending_action: Option<PendingAction>,
    confirm_dialog: Option<ConfirmDialog>,
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
enum ConfirmAction {
    RemoveWorktree { path: String },
    PruneWorktrees,
}

#[derive(Debug)]
pub struct ConfirmDialog {
    pub message: String,
    pub action: ConfirmAction,  
}

impl App {
    pub fn new(repo_path: &str) -> Result<Self> {
        let repo = GitRepo::new(repo_path)?;
        let worktrees = repo.get_worktrees()?;
        let base_path = repo.get_base_path()?;

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

        Ok(Self {
            repo,
            worktrees,
            selected_index,
            terminal_manager,
            terminal_sessions: HashMap::new(),
            active_terminal_path,
            focus: Focus::List,
            base_path,
            should_quit: false,
            add_modal_state: AddWorktreeModal::default(),
            progress_overlay: None,
            error_message: None,
            pending_action: None,
            confirm_dialog: None,
        })
    }

    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        loop {
            // Draw the UI
            terminal.draw(|f| draw::<B>(f, self))?;

            // If the shell exited while focused on the terminal, switch focus back
            // to the list to avoid a "dead" terminal pane capturing input.
            if self.focus == Focus::Terminal && self.terminal_manager.is_disconnected() {
                self.focus = Focus::List;
            }

            self.process_pending_action();

            // Handle events
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key.code, key.modifiers);
                }
            }

            // Update terminal if focused
            if self.focus == Focus::Terminal {
                self.terminal_manager.update().await?;
            }

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

    fn handle_terminal_key(&mut self, key_code: KeyCode, modifiers: KeyModifiers) {
        match key_code {
            KeyCode::Esc => self.focus = Focus::List,
            KeyCode::BackTab => self.focus = Focus::List,
            KeyCode::Char('t') if modifiers.contains(KeyModifiers::CONTROL) => self.focus = Focus::List,
            KeyCode::Char('r') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.terminal_manager.restart();
            }
            _ => {
                // Send key to terminal
                if let Some(input) = self.key_to_ansi(key_code, modifiers) {
                    self.terminal_manager.send_input(&input);
                }
            }
        }
    }

    fn key_to_ansi(&self, key_code: KeyCode, modifiers: KeyModifiers) -> Option<String> {
        match key_code {
            KeyCode::Enter => Some("\r".to_string()),
            KeyCode::Backspace => Some("\x7f".to_string()),
            KeyCode::Delete => Some("\x1b[3~".to_string()),
            KeyCode::Tab => Some("\t".to_string()),
            KeyCode::Esc => Some("\x1b".to_string()),
            KeyCode::Up => Some("\x1b[A".to_string()),
            KeyCode::Down => Some("\x1b[B".to_string()),
            KeyCode::Right => Some("\x1b[C".to_string()),
            KeyCode::Left => Some("\x1b[D".to_string()),
            KeyCode::Home => Some("\x1b[H".to_string()),
            KeyCode::End => Some("\x1b[F".to_string()),
            KeyCode::Char(c) => {
                if modifiers.contains(KeyModifiers::CONTROL) {
                    // Ctrl+key
                    if c.is_ascii_lowercase() {
                        Some(((c as u8 - b'a' + 1) as char).to_string())
                    } else {
                        None
                    }
                } else {
                    Some(c.to_string())
                }
            }
            _ => None,
        }
    }

    fn refresh_worktrees(&mut self) {
        if let Ok(worktrees) = self.repo.get_worktrees() {
            self.worktrees = worktrees;
            // Keep selection valid
            if self.selected_index >= self.worktrees.len() {
                self.selected_index = self.worktrees.len().saturating_sub(1);
            }
        }
    }

    fn remove_selected(&mut self) {
        let path = self
            .worktrees
            .get(self.selected_index)
            .map(|wt| wt.path.clone());

        if let Some(path) = path {
            if self.pending_action.is_none() {
                self.show_progress_overlay("Removing worktree...");
                self.pending_action = Some(PendingAction::RemoveWorktree { path });
            }
        }
    }

    fn prune_worktrees(&mut self) {
        if self.repo.prune_worktrees().is_ok() {
            self.refresh_worktrees();
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
        match key_code {
            KeyCode::Esc => self.close_add_modal(),
            KeyCode::Enter => self.submit_add_modal(),
            KeyCode::Backspace => {
                self.add_modal_state.input.pop();
            }
            KeyCode::Char(c) => {
                if modifiers.contains(KeyModifiers::CONTROL) {
                    return;
                }
                self.add_modal_state.input.push(c);
            }
            KeyCode::Tab => self.add_modal_state.input.push(' '),
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

    fn process_pending_action(&mut self) {
        if let Some(action) = self.pending_action.take() {
            match action {
                PendingAction::AddWorktree { branch } => {
                    match self.repo.add_worktree_from_branch(&branch) {
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
                    match self.repo.remove_worktree(&path) {
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
                    if self.repo.prune_worktrees().is_ok() {
                        self.refresh_worktrees();
                    }
                }
            }

            self.hide_progress_overlay();
        }
    }
}
