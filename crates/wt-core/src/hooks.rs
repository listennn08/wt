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

/// Expand the documented hook variables anywhere they appear.
fn expand(s: &str, ctx: &HookContext) -> String {
    s.replace("${base}", &ctx.base_top.to_string_lossy())
        .replace("${worktree}", &ctx.worktree_path.to_string_lossy())
}

pub fn run_hooks(
    hook_name: &str,
    commands: &[HookCommand],
    ctx: &HookContext,
) -> Result<()> {
    for cmd in commands {
        let args: Vec<String> = cmd
            .args
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|a| expand(a, ctx))
            .collect();

        let cwd = match cmd.cwd.as_deref() {
            None => ctx.base_top.to_path_buf(),
            Some(other) => Path::new(&expand(other, ctx)).to_path_buf(),
        };

        let status = Command::new(&cmd.program)
            .args(&args)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_variables_anywhere_in_a_string() {
        let ctx = HookContext {
            base_top: Path::new("/repo"),
            worktree_path: Path::new("/repo_feat"),
            branch: "feat",
        };
        assert_eq!(expand("${worktree}/frontend", &ctx), "/repo_feat/frontend");
        assert_eq!(expand("--from=${base}", &ctx), "--from=/repo");
        assert_eq!(expand("nothing", &ctx), "nothing");
    }
}
