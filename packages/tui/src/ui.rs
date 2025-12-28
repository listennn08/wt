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
    if input.len() <= max_len {
        input.to_string()
    } else if max_len > 1 {
        format!("{}…", &input[..max_len - 1])
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

    if let Some(msg) = app.progress_overlay() {
        draw_progress_overlay::<B>(f, msg);
    }

    if let Some(message) = app.confirm_message() {
        draw_confirm_dialog::<B>(f, message);
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

    let row_data: Vec<_> = app
        .worktrees
        .iter()
        .enumerate()
        .filter_map(|(idx, wt)| {
            let branch = wt.branch.as_ref()?;
            let branch = truncate_with_ellipsis(branch, MAX_BRANCH_LEN);

            let head = wt
                .head
                .as_ref()
                .map(|head| {
                    if head.len() > 8 {
                        head[..8].to_string()
                    } else {
                        head.clone()
                    }
                })
                .unwrap_or_default();

            let flags = {
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
                    String::new()
                } else {
                    format!("[{}]", flags.join(","))
                }
            };

            Some((idx, branch, head, flags))
        })
        .collect();

    if row_data.is_empty() {
        let list = List::new(Vec::<ListItem>::new())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Worktrees")
                    .border_style(Style::default().fg(Color::White)),
            )
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));
        f.render_widget(list, area);
        return;
    }

    let branch_width = row_data
        .iter()
        .map(|(_, branch, _, _)| branch.len())
        .max()
        .unwrap_or(0);
    let head_width = row_data
        .iter()
        .map(|(_, _, head, _)| head.len())
        .max()
        .unwrap_or(0);
    let flags_width = row_data
        .iter()
        .map(|(_, _, _, flags)| flags.len())
        .max()
        .unwrap_or(0);

    let items: Vec<ListItem> = row_data
        .iter()
        .map(|(idx, branch, head, flags)| {
            let mut style = Style::default();
            if *idx == app.selected_index {
                style = style.fg(Color::Cyan);
            }

            let mut content = vec![];
            if *idx == app.selected_index {
                content.push(Span::styled("> ", Style::default().fg(Color::Cyan)));
            } else {
                content.push(Span::raw("  "));
            }

            let branch_display = if branch_width > 0 {
                format!("{:<width$}", branch, width = branch_width)
            } else {
                branch.clone()
            };
            content.push(Span::styled(branch_display, Style::default().fg(Color::Cyan)));

            if head_width > 0 {
                let head_display = format!(
                    "  {:<width$}",
                    if head.is_empty() { "" } else { head },
                    width = head_width
                );
                content.push(Span::styled(head_display, Style::default().fg(Color::Yellow)));
            }

            if flags_width > 0 {
                let flags_display = if flags.is_empty() {
                    " ".repeat(flags_width + 2)
                } else {
                    format!("  {:<width$}", flags, width = flags_width)
                };
                content.push(Span::styled(flags_display, Style::default().fg(Color::Magenta)));
            }

            ListItem::new(Line::from(content)).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Worktrees")
                .border_style(Style::default().fg(Color::White)),
        )
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));

    f.render_widget(list, area);
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

    let paragraph = Paragraph::new(display_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Terminal")
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
            let display_err = if err.len() > MAX_ERR_LEN {
                format!("{}…", &err[..MAX_ERR_LEN - 1])
            } else {
                err.clone()
            };
            lines.push(Line::from(Span::styled(
                display_err,
                Style::default().fg(Color::LightRed),
            )));
        } else {
            // Reserve vertical space even when no error is present so the hint
            // always appears at the same position.
            lines.push(Line::from(" "));
        }

        lines.push(Line::from(" "));
        lines.push(Line::from(Span::styled(
            "Enter to create · Esc to cancel",
            Style::default().fg(Color::Gray),
        )));
    }
    // always leave hint at bottom

    let block = Block::default()
        .title("Add Worktree")
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
            "Pressing keys won’t have effect until this finishes.",
            Style::default().fg(Color::Gray),
        )),
    ])
    .alignment(Alignment::Center)
    .block(block);

    f.render_widget(Clear, modal_area);
    f.render_widget(paragraph, modal_area);
}
