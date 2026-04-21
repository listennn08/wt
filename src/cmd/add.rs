use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use wt_core::git::GitRepo;
use wt_core::types::AddOptions;
use wt_core::worktree;

use crate::output;

#[derive(Args, Debug)]
pub struct AddArgs {
    /// Branch to create/use for worktree
    branch: String,

    /// Target directory for the worktree
    #[arg(short, long)]
    dir: Option<PathBuf>,

    /// Force creating a new branch even if remote branch exists
    #[arg(short = 'n', long)]
    new_branch: bool,

    /// Base ref when creating a new branch (default: current branch)
    #[arg(short, long)]
    base: Option<String>,

    /// Remote name (default: origin)
    #[arg(short, long, default_value = "origin")]
    remote: String,

    /// Allow if target directory already exists
    #[arg(short, long)]
    force: bool,

    /// Do not print step/progress messages
    #[arg(long)]
    no_progress: bool,
}

pub fn run(args: AddArgs) -> Result<()> {
    let repo = GitRepo::open(&std::env::current_dir()?)?;
    let progress = !args.no_progress;

    let opts = AddOptions {
        branch: args.branch,
        dir: args.dir,
        new_branch: args.new_branch,
        base: args.base,
        remote: args.remote,
        force: args.force,
        progress,
    };

    let path = worktree::add_worktree(&repo, opts)?;
    if progress {
        output::log(&path.to_string_lossy());
    } else {
        println!("{}", path.display());
    }
    Ok(())
}
