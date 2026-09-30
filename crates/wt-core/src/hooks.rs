// crates/wt-core/src/hooks.rs
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{anyhow, Result};

use crate::config::HookCommand;

pub struct HookContext<'a> {
    pub base_top: &'a Path,
    pub worktree_path: &'a Path,
    pub branch: &'a str,
    pub capture_output: bool,
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

        let mut command = Command::new(&cmd.program);
        command
            .args(&args)
            .current_dir(&cwd)
            .env("WT_BASE", ctx.base_top.as_os_str())
            .env("WT_WORKTREE", ctx.worktree_path.as_os_str())
            .env("WT_BRANCH", ctx.branch);
        let result = if ctx.capture_output {
            command.stdin(Stdio::null()).output().map(|o| (o.status, o.stderr))
        } else {
            command.status().map(|s| (s, Vec::new()))
        };
        let (status, stderr) = result.map_err(|e| {
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
            let mut msg = format!(
                "Hook {} failed: cmd='{}' cwd='{}' exitCode={}",
                hook_name,
                cmdline,
                cwd.display(),
                status.code().map_or("null".to_string(), |c| c.to_string())
            );
            let stderr = String::from_utf8_lossy(&stderr);
            let tail: Vec<&str> = stderr.trim_end().lines().rev().take(10).collect();
            for line in tail.into_iter().rev() {
                msg.push('\n');
                msg.push_str(line);
            }
            return Err(anyhow!(msg));
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
            capture_output: false,
        };
        assert_eq!(expand("${worktree}/frontend", &ctx), "/repo_feat/frontend");
        assert_eq!(expand("--from=${base}", &ctx), "--from=/repo");
        assert_eq!(expand("nothing", &ctx), "nothing");
    }

    #[test]
    fn captured_hook_failure_reports_its_stderr() {
        let dir = std::env::temp_dir();
        let ctx = HookContext {
            base_top: &dir,
            worktree_path: &dir,
            branch: "feat",
            capture_output: true,
        };
        let hook = HookCommand {
            program: "sh".into(),
            args: Some(vec!["-c".into(), "echo noise; echo boom >&2; exit 3".into()]),
            cwd: None,
        };
        let err = run_hooks("hooks.add.post_create", &[hook], &ctx).unwrap_err().to_string();
        assert!(err.contains("exitCode=3"), "{err}");
        assert!(err.ends_with("\nboom"), "{err}");
        assert_eq!(err.lines().count(), 2, "stdout must not be included: {err}");
    }
}
