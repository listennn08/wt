use std::io;

use anyhow::Result;
use clap::Args;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use wt_tui::app::App;

#[derive(Args, Debug)]
pub struct TuiArgs {
    /// Repository path
    #[arg(short, long, default_value = ".")]
    repo: String,
}

pub fn run(args: TuiArgs) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Never `?` before the terminal is restored, or a failure leaves the user's
    // shell in raw mode on the alternate screen.
    let res = App::new(&args.repo).and_then(|mut app| app.run(&mut terminal));

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()?;

    res
}
