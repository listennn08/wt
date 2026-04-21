mod cmd;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "wt", about = "Git worktree manager", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new worktree from a branch
    Add(cmd::add::AddArgs),
    /// List all worktrees
    List(cmd::list::ListArgs),
    /// Delete a worktree
    #[command(alias = "rm")]
    Remove(cmd::remove::RemoveArgs),
    /// Switch to a worktree and open a shell in its directory
    #[command(alias = "sw")]
    Switch(cmd::switch::SwitchArgs),
    /// Prune stale worktree information
    Prune(cmd::prune::PruneArgs),
    /// Interactive TUI for worktrees
    Tui(cmd::tui::TuiArgs),
    /// Remove wt shell completions and print uninstall instructions
    Uninstall(cmd::uninstall::UninstallArgs),
    /// Shell completion utilities
    Completion(cmd::completion::CompletionArgs),

    // Hidden completion helpers
    #[command(name = "__complete-branches", hide = true)]
    CompleteBranches,
    #[command(name = "__complete-worktrees", hide = true)]
    CompleteWorktrees,
    #[command(name = "__complete-actions", hide = true)]
    CompleteActions,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Add(args) => cmd::add::run(args),
        Commands::List(args) => cmd::list::run(args),
        Commands::Remove(args) => cmd::remove::run(args),
        Commands::Switch(args) => cmd::switch::run(args),
        Commands::Prune(args) => cmd::prune::run(args),
        Commands::Tui(args) => cmd::tui::run(args),
        Commands::Uninstall(args) => cmd::uninstall::run(args),
        Commands::Completion(args) => cmd::completion::run(args),
        Commands::CompleteBranches => cmd::completion::complete_branches(),
        Commands::CompleteWorktrees => cmd::completion::complete_worktrees(),
        Commands::CompleteActions => { cmd::completion::complete_actions(); Ok(()) }
    }
}
