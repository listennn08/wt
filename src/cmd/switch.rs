use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Result};
use clap::Args;
use wt_core::git::GitRepo;
use wt_core::worktree;

#[derive(Args, Debug)]
pub struct SwitchArgs {
    /// Worktree path or branch name
    target: String,

    /// Treat target as a branch name
    #[arg(short, long)]
    branch: bool,

    /// Treat target as a worktree path
    #[arg(short, long)]
    path: bool,

    /// Print resolved worktree path only
    #[arg(long)]
    print: bool,

    /// Shell to use (default: $SHELL)
    #[arg(long)]
    shell: Option<String>,
}

pub fn run(args: SwitchArgs) -> Result<()> {
    let repo = GitRepo::open(&std::env::current_dir()?)?;

    let resolved = if args.path {
        let p = Path::new(&args.target);
        p.canonicalize().unwrap_or(p.to_path_buf())
    } else {
        worktree::resolve_switch_target(&repo, &args.target)?
    };

    if args.print {
        println!("{}", resolved.display());
        return Ok(());
    }

    let shell = args
        .shell
        .or_else(|| std::env::var("SHELL").ok())
        .ok_or_else(|| anyhow!("No shell found. Provide --shell or set $SHELL"))?;

    let status = Command::new(&shell)
        .current_dir(&resolved)
        .status()
        .map_err(|e| anyhow!("Failed to start shell: {}", e))?;

    std::process::exit(status.code().unwrap_or(1));
}
