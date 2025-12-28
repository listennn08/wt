mod app;
mod git;
mod terminal;
mod ui;

use std::io;

use anyhow::Result;
use clap::Parser;
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::App;
use crate::git::GitRepo;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Repository path
    #[arg(short, long, default_value = ".")]
    repo: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Test git functionality first
    test_git()?;

    println!("\nStarting TUI...");
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run it
    let mut app = App::new(&args.repo)?;
    let res = app.run(&mut terminal).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        println!("Error: {:?}", err);
    }

    Ok(())
}

fn test_git() -> Result<()> {
    let args = Args::parse();

    let git_repo = GitRepo::new(&args.repo)?;
    let worktrees = git_repo.get_worktrees()?;

    println!("Found {} worktrees:", worktrees.len());
    for (i, wt) in worktrees.iter().enumerate() {
        println!("  {}: path={}, branch={:?}, head={:?}, base={}",
                i, wt.path, wt.branch, wt.head, wt.is_base);
    }

    Ok(())
}
