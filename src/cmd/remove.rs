use anyhow::Result;
use clap::Args;
use wt_core::git::GitRepo;
use wt_core::types::RemoveOptions;
use wt_core::worktree;

#[derive(Args, Debug)]
pub struct RemoveArgs {
    /// Worktree path or branch name
    target: String,

    /// Treat target as a branch name
    #[arg(short, long)]
    branch: bool,

    /// Treat target as a worktree path
    #[arg(short, long)]
    path: bool,

    /// Force removal
    #[arg(short, long)]
    force: bool,
}

pub fn run(args: RemoveArgs) -> Result<()> {
    let repo = GitRepo::open(&std::env::current_dir()?)?;
    let opts = RemoveOptions {
        target: args.target,
        force: args.force,
        as_branch: args.branch,
        as_path: args.path,
    };
    let resolved = worktree::remove_worktree(&repo, opts)?;
    println!("{}", resolved.display());
    Ok(())
}
