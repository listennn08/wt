use ratatui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, Focus};
use vt100::Color as VtColor;

fn truncate_with_ellipsis(input: &str, max_len: usize) -> String {
    // Count chars, not bytes: a byte-index slice panics mid-UTF8 on a branch
    // name or path containing non-ASCII.
    if input.chars().count() <= max_len {
        input.to_string()
    } else if max_len > 1 {
        let head: String = input.chars().take(max_len - 1).collect();
        format!("{}…", head)
    } else {
        "…".to_string()
    }
}

pub fn draw<B: Backend>(f: &mut Frame, app: &mut App) {
    let size = f.size();

    // Main layout: title + content + status
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),     // Title
            Constraint::Min(10),       // Content
            Constraint::Length(2),     // Status
            Constraint::Length(1),     // Key hints
        ])
        .split(size);

    // Title
    let title = Paragraph::new("wt tui")
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .alignment(Alignment::Left);
    f.render_widget(title, chunks[0]);

    // Content area
    draw_content::<B>(f, app, chunks[1]);

    // Status bar
    draw_status::<B>(f, app, chunks[2]);

    // Key hints
    draw_hints::<B>(f, app, chunks[3]);

    if let Some(msg) = app.progress_overlay() {
        draw_progress_overlay::<B>(f, msg);
    }

    if let Some(message) = app.confirm_message() {
        draw_confirm_dialog::<B>(f, message);
    }

    // Errors were being stored and never shown: the operation just appeared to
    // do nothing, and the next keypress got swallowed dismissing the invisible
    // message.
    if let Some(message) = app.error_message() {
        draw_error_dialog::<B>(f, message);
    }

    if app.add_modal_visible() {
        draw_add_worktree_modal::<B>(f, app);
    }
}

fn draw_content<B: Backend>(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(50),   // Worktrees list (fixed)
            Constraint::Fill(1),      // Terminal/Details take the rest
        ])
        .split(area);

    // Worktrees list
    draw_worktrees_list::<B>(f, app, chunks[0]);

    // Terminal or details
    if app.focus == Focus::Terminal {
        draw_terminal::<B>(f, app, chunks[1]);
    } else {
        draw_details::<B>(f, app, chunks[1]);
    }
}

fn draw_worktrees_list<B: Backend>(f: &mut Frame, app: &mut App, area: Rect) {
    const MAX_BRANCH_LEN: usize = 24;

    let rows: Vec<(String, String, String)> = app
        .worktrees
        .iter()
        .map(|wt| {
            // Every worktree gets a row. Skipping the branch-less ones used to
            // desync this list from selected_index, which indexes app.worktrees.
            let branch = match wt.branch.as_deref() {
                Some(branch) => truncate_with_ellipsis(branch, MAX_BRANCH_LEN),
                None if wt.detached == Some(true) => "(detached)".to_string(),
                None => "(no branch)".to_string(),
            };

            let head: String = wt
                .head
                .as_deref()
                .unwrap_or_default()
                .chars()
                .take(8)
                .collect();

            let mut flags = vec![];
            if wt.is_base {
                flags.push("base");
            }
            if wt.is_locked {
                flags.push("locked");
            }
            if wt.is_prunable {
                flags.push("prunable");
            }
            let flags = if flags.is_empty() {
                String::new()
            } else {
                format!("[{}]", flags.join(","))
            };

            (branch, head, flags)
        })
        .collect();

    let branch_width = rows
        .iter()
        .map(|(branch, _, _)| branch.chars().count())
        .max()
        .unwrap_or(0);
    let head_width = rows
        .iter()
        .map(|(_, head, _)| head.chars().count())
        .max()
        .unwrap_or(0);

    let items: Vec<ListItem> = rows
        .iter()
        .map(|(branch, head, flags)| {
            let mut content = vec![Span::styled(
                format!("{:<width$}", branch, width = branch_width),
                Style::default().fg(Color::Cyan),
            )];
            if head_width > 0 {
                content.push(Span::styled(
                    format!("  {:<width$}", head, width = head_width),
                    Style::default().fg(Color::Yellow),
                ));
            }
            if !flags.is_empty() {
                content.push(Span::styled(
                    format!("  {}", flags),
                    Style::default().fg(Color::Magenta),
                ));
            }
            ListItem::new(Line::from(content))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Worktrees ({})", app.worktrees.len()))
                .border_style(Style::default().fg(Color::White)),
        )
        // A stateful list scrolls to keep the selection on screen; the old
        // stateless render just clipped everything past the pane height.
        .highlight_symbol("> ")
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );

    let selected = (!app.worktrees.is_empty()).then_some(app.selected_index);
    app.list_state.select(selected);
    f.render_stateful_widget(list, area, &mut app.list_state);
}

fn draw_terminal<B: Backend>(f: &mut Frame, app: &mut App, area: Rect) {
    // Render from a terminal emulator buffer so ANSI clear/cursor control
    // only affects this region.
    let inner_height = area.height.saturating_sub(2).max(1);
    let inner_width = area.width.saturating_sub(2).max(1);

    // Resize the PTY (and underlying parser) to match the visible area so that
    // the shell uses the full available width.
    app.terminal_manager.resize(inner_width, inner_height);

    let rows = app
        .terminal_manager
        .get_screen_cells(inner_height, inner_width);

    fn vt_color_to_ratatui(c: VtColor) -> Option<Color> {
        match c {
            VtColor::Default => None,
            VtColor::Rgb(r, g, b) => Some(Color::Rgb(r, g, b)),
            VtColor::Idx(i) => {
                // Map basic 0-15 to ratatui's named colors; fall back to None
                // for the rest (to avoid incorrect palettes).
                match i {
                    0 => Some(Color::Black),
                    1 => Some(Color::Red),
                    2 => Some(Color::Green),
                    3 => Some(Color::Yellow),
                    4 => Some(Color::Blue),
                    5 => Some(Color::Magenta),
                    6 => Some(Color::Cyan),
                    7 => Some(Color::Gray),
                    8 => Some(Color::DarkGray),
                    9 => Some(Color::LightRed),
                    10 => Some(Color::LightGreen),
                    11 => Some(Color::LightYellow),
                    12 => Some(Color::LightBlue),
                    13 => Some(Color::LightMagenta),
                    14 => Some(Color::LightCyan),
                    15 => Some(Color::White),
                    _ => None,
                }
            }
        }
    }

    let mut display_lines: Vec<Line> = Vec::with_capacity(rows.len());
    for row in rows {
        let mut spans: Vec<Span> = Vec::new();
        let mut cur_style: Option<Style> = None;
        let mut cur_text = String::new();

        for cell in row {
            let mut style = Style::default();

            let fg = vt_color_to_ratatui(cell.fgcolor());
            let bg = vt_color_to_ratatui(cell.bgcolor());

            if cell.inverse() {
                // Swap if inverse
                if let Some(bg) = bg {
                    style = style.fg(bg);
                }
                if let Some(fg) = fg {
                    style = style.bg(fg);
                }
            } else {
                if let Some(fg) = fg {
                    style = style.fg(fg);
                }
                if let Some(bg) = bg {
                    style = style.bg(bg);
                }
            }

            if cell.bold() {
                style = style.add_modifier(Modifier::BOLD);
            }
            if cell.italic() {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if cell.underline() {
                style = style.add_modifier(Modifier::UNDERLINED);
            }

            let ch = if cell.has_contents() {
                cell.contents().to_string()
            } else {
                " ".to_string()
            };

            if let Some(cs) = cur_style {
                if cs == style {
                    cur_text.push_str(&ch);
                } else {
                    spans.push(Span::styled(cur_text.clone(), cs));
                    cur_text.clear();
                    cur_text.push_str(&ch);
                    cur_style = Some(style);
                }
            } else {
                cur_style = Some(style);
                cur_text.push_str(&ch);
            }
        }

        if let Some(cs) = cur_style {
            spans.push(Span::styled(cur_text, cs));
        }

        display_lines.push(Line::from(spans));
    }

    let label = app
        .worktrees
        .get(app.selected_index)
        .map(|wt| match wt.branch.as_deref() {
            Some(branch) => branch.to_string(),
            None => std::path::Path::new(&wt.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| wt.path.clone()),
        })
        .unwrap_or_default();
    let title = if app.terminal_manager.is_scrolled_back() {
        format!("Terminal — {} ↑ scrollback", label)
    } else if label.is_empty() {
        "Terminal".to_string()
    } else {
        format!("Terminal — {}", label)
    };

    let paragraph = Paragraph::new(display_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(if app.focus == Focus::Terminal {
                    Color::Cyan
                } else {
                    Color::White
                })),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn draw_details<B: Backend>(f: &mut Frame, app: &mut App, area: Rect) {
    let content = if let Some(wt) = app.worktrees.get(app.selected_index) {
        vec![
            Line::from(vec![
                Span::styled("Path: ", Style::default().fg(Color::Gray)),
                Span::raw(&wt.path),
            ]),
            Line::from(vec![
                Span::styled("Branch: ", Style::default().fg(Color::Gray)),
                Span::raw(wt.branch.as_deref().unwrap_or("(none)")),
            ]),
            Line::from(vec![
                Span::styled("Head: ", Style::default().fg(Color::Gray)),
                Span::raw(wt.head.as_deref().unwrap_or("(unknown)")),
            ]),
            Line::from(vec![
                Span::styled("Flags: ", Style::default().fg(Color::Gray)),
                Span::raw({
                    let mut flags = vec![];
                    if wt.is_base {
                        flags.push("base");
                    }
                    if wt.is_locked {
                        flags.push("locked");
                    }
                    if wt.is_prunable {
                        flags.push("prunable");
                    }
                    if flags.is_empty() {
                        "(none)".to_string()
                    } else {
                        flags.join(", ")
                    }
                }),
            ]),
        ]
    } else {
        vec![Line::from("No selection")]
    };

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Details")
                .border_style(Style::default().fg(Color::White)),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn draw_status<B: Backend>(f: &mut Frame, app: &mut App, area: Rect) {
    const STATUS_PATH_MAX: usize = 60;
    const STATUS_BRANCH_MAX: usize = 28;

    let mut info_spans = Vec::new();
    if let Some(wt) = app.worktrees.get(app.selected_index) {
        info_spans.push(Span::styled("path ", Style::default().fg(Color::Gray)));
        info_spans.push(Span::styled(
            truncate_with_ellipsis(&wt.path, STATUS_PATH_MAX),
            Style::default().fg(Color::LightBlue),
        ));
        info_spans.push(Span::raw("  "));

        info_spans.push(Span::styled("branch ", Style::default().fg(Color::Gray)));
        info_spans.push(Span::styled(
            truncate_with_ellipsis(
                wt.branch.as_deref().unwrap_or("(none)"),
                STATUS_BRANCH_MAX,
            ),
            Style::default().fg(Color::LightCyan),
        ));
        info_spans.push(Span::raw("  "));

        info_spans.push(Span::styled("head ", Style::default().fg(Color::Gray)));
        info_spans.push(Span::styled(
            wt.head
                .as_deref()
                .map(|h| if h.len() > 8 { &h[..8] } else { h })
                .unwrap_or("(unknown)"),
            Style::default().fg(Color::Yellow),
        ));
        info_spans.push(Span::raw("  "));

        let mut flags = vec![];
        if wt.is_base {
            flags.push("base");
        }
        if wt.is_locked {
            flags.push("locked");
        }
        if wt.is_prunable {
            flags.push("prunable");
        }
        info_spans.push(Span::styled("flags ", Style::default().fg(Color::Gray)));
        info_spans.push(Span::styled(
            if flags.is_empty() {
                "(none)".to_string()
            } else {
                flags.join(", ")
            },
            Style::default().fg(Color::Magenta),
        ));
    } else {
        info_spans.push(Span::styled(
            "No selection",
            Style::default().fg(Color::Gray),
        ));
    }

    let status = Paragraph::new(Line::from(info_spans))
        .block(Block::default().borders(Borders::TOP));

    f.render_widget(status, area);
}

/// Bottom key hints. Modals draw their own, so this only covers the two
/// focus modes.
fn draw_hints<B: Backend>(f: &mut Frame, app: &mut App, area: Rect) {
    let hints: &[(&str, &str)] = if app.focus == Focus::Terminal {
        if app.terminal_manager.is_scrolled_back() {
            &[("esc", "list"), ("any key", "back to live")]
        } else {
            &[
                ("esc", "list"),
                ("^r", "restart shell"),
                ("scroll", "history"),
            ]
        }
    } else {
        &[
            ("↑↓", "move"),
            ("⏎", "terminal"),
            ("a", "add"),
            ("r", "remove"),
            ("x", "prune"),
            ("g", "refresh"),
            ("R", "restart"),
            ("q", "quit"),
        ]
    };

    let key_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);
    let label_style = Style::default().fg(Color::DarkGray);

    let mut spans = Vec::with_capacity(hints.len() * 4);
    for (i, (key, label)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" · ", label_style));
        }
        spans.push(Span::styled(*key, key_style));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(*label, label_style));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_error_dialog<B: Backend>(f: &mut Frame, message: &str) {
    let area = f.size();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Length(9),
            Constraint::Percentage(30),
        ])
        .split(area);

    let modal_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(15),
            Constraint::Fill(1),
            Constraint::Percentage(15),
        ])
        .split(vertical[1])[1];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .style(Style::default().bg(Color::Black))
        .title("Error");

    let paragraph = Paragraph::new(vec![
        Line::from(Span::styled(
            message.trim().to_string(),
            Style::default().fg(Color::White),
        )),
        Line::from(" "),
        Line::from(Span::styled(
            "press any key to dismiss",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .wrap(Wrap { trim: true })
    .block(block);

    f.render_widget(Clear, modal_area);
    f.render_widget(paragraph, modal_area);
}

fn draw_add_worktree_modal<B: Backend>(f: &mut Frame, app: &mut App) {
    let area = f.size();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Length(7),
            Constraint::Percentage(40),
        ])
        .split(area);

    let modal_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(20),
            Constraint::Fill(1),
            Constraint::Percentage(20),
        ])
        .split(vertical[1])[1];

    let inner_width = modal_area.width.saturating_sub(2) as usize;
    let matches = app.branch_matches();
    let match_count = matches.len();
    let modal = app.add_modal();
    let mut lines = Vec::new();

    if modal.is_submitting {
        lines.push(Line::from(Span::styled(
            "Creating worktree...",
            Style::default()
                .fg(Color::LightCyan)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(" "));
        lines.push(Line::from(" "));
        lines.push(Line::from(" "));
    } else {
        lines.push(Line::from(" "));
        lines.push(Line::from(vec![
            Span::styled("Branch: ", Style::default().fg(Color::Gray)),
            Span::styled(
                format!("{}_", modal.input),
                Style::default().fg(Color::White),
            ),
        ]));

        if let Some(err) = &modal.error {
            const MAX_ERR_LEN: usize = 80;
            lines.push(Line::from(Span::styled(
                truncate_with_ellipsis(err, MAX_ERR_LEN),
                Style::default().fg(Color::LightRed),
            )));
        } else if match_count == 0 {
            lines.push(Line::from(" "));
        } else {
            // Kept to a single line: wrapping would push the hint out of the modal.
            let shown = truncate_with_ellipsis(&matches.join("  "), inner_width);
            lines.push(Line::from(Span::styled(
                shown,
                Style::default().fg(Color::DarkGray),
            )));
        }

        lines.push(Line::from(" "));
        lines.push(Line::from(Span::styled(
            "Enter create · Tab complete · ^w del word · ^u clear · Esc cancel",
            Style::default().fg(Color::Gray),
        )));
    }
    // always leave hint at bottom

    let title = if match_count > 0 && modal.error.is_none() && !modal.is_submitting {
        format!("Add Worktree ({} matches)", match_count)
    } else {
        "Add Worktree".to_string()
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .style(Style::default().bg(Color::Black));

    let paragraph = Paragraph::new(lines)
        .alignment(Alignment::Left)
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(Clear, modal_area);
    f.render_widget(paragraph, modal_area);
}

fn draw_confirm_dialog<B: Backend>(f: &mut Frame, message: &str) {
    let area = f.size();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Length(6),
            Constraint::Percentage(40),
        ])
        .split(area);

    let modal_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Fill(1),
            Constraint::Percentage(25),
        ])
        .split(vertical[1])[1];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black))
        .title("Confirm");

    let paragraph = Paragraph::new(vec![
        Line::from(" "),
        Line::from(Span::styled(
            message,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(" "),
        Line::from(Span::styled(
            "[Enter / y] Yes   [Esc / n] Cancel",
            Style::default().fg(Color::Gray),
        )),
    ])
    .alignment(Alignment::Center)
    .block(block);

    f.render_widget(Clear, modal_area);
    f.render_widget(paragraph, modal_area);
}

fn draw_progress_overlay<B: Backend>(f: &mut Frame, message: &str) {
    let area = f.size();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Length(5),
            Constraint::Percentage(40),
        ])
        .split(area);

    let modal_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Fill(1),
            Constraint::Percentage(25),
        ])
        .split(vertical[1])[1];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .style(Style::default().bg(Color::Black))
        .title("Please wait");

    let paragraph = Paragraph::new(vec![
        Line::from(" "),
        Line::from(Span::styled(
            message,
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )),
        Line::from(" "),
        Line::from(Span::styled(
            "Pressing keys won't have effect until this finishes.",
            Style::default().fg(Color::Gray),
        )),
    ])
    .alignment(Alignment::Center)
    .block(block);

    f.render_widget(Clear, modal_area);
    f.render_widget(paragraph, modal_area);
}
