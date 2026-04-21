// crates/wt-core/src/hooks.rs
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Result};

use crate::config::HookCommand;

pub struct HookContext<'a> {
    pub base_top: &'a Path,
    pub worktree_path: &'a Path,
    pub branch: &'a str,
}

pub fn run_hooks(
    hook_name: &str,
    commands: &[HookCommand],
    ctx: &HookContext,
) -> Result<()> {
    for cmd in commands {
        let args = cmd.args.as_deref().unwrap_or(&[]);

        let cwd = match cmd.cwd.as_deref() {
            None | Some("${base}") => ctx.base_top.to_path_buf(),
            Some("${worktree}") => ctx.worktree_path.to_path_buf(),
            Some(other) => Path::new(other).to_path_buf(),
        };

        let status = Command::new(&cmd.program)
            .args(args)
            .current_dir(&cwd)
            .env("WT_BASE", ctx.base_top.as_os_str())
            .env("WT_WORKTREE", ctx.worktree_path.as_os_str())
            .env("WT_BRANCH", ctx.branch)
            .status()
            .map_err(|e| {
                anyhow!(
                    "Hook {} failed to start: {}: {}",
                    hook_name,
                    cmd.program,
                    e
                )
            })?;

        if !status.success() {
            let cmdline = std::iter::once(cmd.program.as_str())
                .chain(args.iter().map(|s| s.as_str()))
                .collect::<Vec<_>>()
                .join(" ");
            return Err(anyhow!(
                "Hook {} failed: cmd='{}' cwd='{}' exitCode={}",
                hook_name,
                cmdline,
                cwd.display(),
                status.code().map_or("null".to_string(), |c| c.to_string())
            ));
        }
    }
    Ok(())
}
