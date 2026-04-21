use anyhow::Result;
use clap::Args;
use wt_core::git::GitRepo;
use wt_core::types::PruneOptions;
use wt_core::worktree;

#[derive(Args, Debug)]
pub struct PruneArgs {
    /// Do not remove anything; show what would be pruned
    #[arg(long)]
    dry_run: bool,

    /// Report all removals
    #[arg(long)]
    verbose: bool,

    /// Expire worktrees older than <time>
    #[arg(long)]
    expire: Option<String>,
}

pub fn run(args: PruneArgs) -> Result<()> {
    let repo = GitRepo::open(&std::env::current_dir()?)?;
    let opts = PruneOptions {
        dry_run: args.dry_run,
        verbose: args.verbose,
        expire: args.expire,
    };
    let output = worktree::prune_worktrees(&repo, opts)?;
    if !output.is_empty() {
        print!("{}", output);
    }
    Ok(())
}
