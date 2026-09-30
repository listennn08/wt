// crates/wt-core/src/git.rs
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};

use crate::types::WorktreeInfo;

pub struct GitRepo {
    root: PathBuf,
}

impl GitRepo {
    /// Opens the repo rooted at its main worktree, even when `path` is inside a
    /// linked worktree — new worktrees, config, and env files all hang off it.
    pub fn open(path: &Path) -> Result<Self> {
        let output = Command::new("git")
            .args([
                "rev-parse",
                "--path-format=absolute",
                "--git-common-dir",
                "--show-toplevel",
            ])
            .current_dir(path)
            .output()
            .with_context(|| format!("Failed to run git in {}", path.display()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut lines = stdout.lines().map(str::trim);
        match (output.status.success(), lines.next(), lines.next()) {
            (true, Some(common_dir), Some(toplevel)) if !toplevel.is_empty() => Ok(Self {
                root: main_worktree_root(Path::new(common_dir), Path::new(toplevel)),
            }),
            _ => Err(anyhow!("Not a git repository: {}", path.display())),
        }
    }

    /// Run git in the repo root, erroring with stderr on a non-zero exit.
    fn git(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
        if !output.status.success() {
            return Err(anyhow!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub fn repo_root(&self) -> Result<PathBuf> {
        Ok(self.root.clone())
    }

    pub fn repo_name(&self) -> Result<String> {
        self.root
            .file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Cannot determine repository name"))
    }

    pub fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>> {
        let output = self.git(&["worktree", "list", "--porcelain"])?;
        let mut worktrees = parse_worktree_list(&output);
        worktrees.sort_by(|a, b| match (a.is_base, b.is_base) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.path.cmp(&b.path),
        });
        Ok(worktrees)
    }

    pub fn branch_exists_local(&self, branch: &str) -> bool {
        // Must be an actual branch: revspec matching would also accept tags,
        // remote refs and things like HEAD~2, which produce a detached checkout.
        Command::new("git")
            .args(["show-ref", "--verify", "--quiet"])
            .arg(format!("refs/heads/{}", branch))
            .current_dir(&self.root)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn branch_exists_remote(&self, remote: &str, branch: &str) -> Result<bool> {
        let output = Command::new("git")
            .args(["ls-remote", "--heads", remote, branch])
            .current_dir(&self.root)
            .output()
            .context("Failed to run git ls-remote")?;
        Ok(output.status.success() && !output.stdout.is_empty())
    }

    pub fn current_branch(&self) -> Option<String> {
        let output = Command::new("git")
            .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
            .current_dir(&self.root)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (!branch.is_empty()).then_some(branch)
    }

    pub fn default_worktree_dir(&self, branch: &str) -> Result<PathBuf> {
        let parent = self
            .root
            .parent()
            .ok_or_else(|| anyhow!("Cannot determine repository parent directory"))?;
        let name = self.repo_name()?;
        Ok(parent.join(format!("{}_{}", name, sanitize_branch_name(branch))))
    }

    // These capture git's output rather than inheriting stdio: inherited output
    // scribbles over the TUI's alternate screen, and the captured stderr makes
    // for a far better error than "git worktree add failed".
    pub fn git_worktree_add(&self, args: &[&str]) -> Result<()> {
        let mut argv = vec!["worktree", "add"];
        argv.extend_from_slice(args);
        self.git(&argv).map(|_| ())
    }

    pub fn git_worktree_remove(&self, path: &str, force: bool) -> Result<()> {
        let mut argv = vec!["worktree", "remove"];
        if force {
            argv.push("--force");
        }
        argv.push(path);
        self.git(&argv).map(|_| ())
    }

    pub fn git_worktree_prune(
        &self,
        dry_run: bool,
        verbose: bool,
        expire: Option<&str>,
    ) -> Result<String> {
        let mut args: Vec<String> = vec!["worktree".into(), "prune".into()];
        if dry_run {
            args.push("--dry-run".into());
        }
        if verbose {
            args.push("--verbose".into());
        }
        if let Some(expire) = expire {
            args.push(format!("--expire={}", expire));
        }
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        self.git(&refs)
    }

    pub fn list_branches(&self) -> Result<Vec<String>> {
        let output = self.git(&[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes",
        ])?;
        let mut branches: Vec<String> = output
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|b| !b.is_empty() && !b.ends_with("/HEAD"))
            .collect();
        branches.sort();
        branches.dedup();
        Ok(branches)
    }

    /// Branch names in the form `wt add` accepts: local branches, plus remote
    /// branches with their `<remote>/` prefix stripped (lstrip=3 drops
    /// `refs/remotes/<remote>`, so `feature/x` survives intact).
    pub fn list_branch_candidates(&self) -> Result<Vec<String>> {
        let local = self.git(&["for-each-ref", "--format=%(refname:short)", "refs/heads"])?;
        let remote = self.git(&["for-each-ref", "--format=%(refname:lstrip=3)", "refs/remotes"])?;

        let mut names: Vec<String> = local
            .lines()
            .chain(remote.lines())
            .map(|l| l.trim().to_string())
            .filter(|b| !b.is_empty() && b != "HEAD")
            .collect();
        names.sort();
        names.dedup();
        Ok(names)
    }

    pub fn list_worktree_paths(&self) -> Result<Vec<String>> {
        Ok(self
            .list_worktrees()?
            .into_iter()
            .filter(|wt| !wt.is_base)
            .map(|wt| wt.path)
            .collect())
    }

    /// Path of the worktree currently checking out `branch`, if any.
    pub fn worktree_path_for_branch(&self, branch: &str) -> Result<Option<PathBuf>> {
        Ok(self
            .list_worktrees()?
            .into_iter()
            .find(|wt| wt.branch.as_deref() == Some(branch))
            .map(|wt| PathBuf::from(wt.path)))
    }

    pub fn resolve_worktree_path(&self, target: &str) -> Result<Option<PathBuf>> {
        let as_path = Path::new(target);
        if as_path.exists() {
            return Ok(Some(as_path.canonicalize().unwrap_or(as_path.to_path_buf())));
        }

        if let Some(path) = self.worktree_path_for_branch(target)? {
            return Ok(Some(path));
        }

        let fallback = self.default_worktree_dir(target)?;
        if fallback.exists() {
            return Ok(Some(fallback));
        }

        Ok(None)
    }
}

/// Parse `git worktree list --porcelain`. The first entry is the main worktree.
pub fn parse_worktree_list(output: &str) -> Vec<WorktreeInfo> {
    let mut result: Vec<WorktreeInfo> = Vec::new();
    let mut current: Option<WorktreeInfo> = None;

    for line in output.lines() {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => {
                if let Some(wt) = current.take() {
                    result.push(wt);
                }
                current = Some(WorktreeInfo {
                    path: value.to_string(),
                    branch: None,
                    head: None,
                    is_base: result.is_empty(),
                    is_locked: false,
                    is_prunable: false,
                    detached: None,
                });
            }
            "HEAD" => {
                if let Some(wt) = current.as_mut() {
                    wt.head = Some(value.to_string());
                }
            }
            "branch" => {
                if let Some(wt) = current.as_mut() {
                    wt.branch =
                        Some(value.strip_prefix("refs/heads/").unwrap_or(value).to_string());
                }
            }
            "detached" => {
                if let Some(wt) = current.as_mut() {
                    wt.detached = Some(true);
                }
            }
            "locked" => {
                if let Some(wt) = current.as_mut() {
                    wt.is_locked = true;
                }
            }
            "prunable" => {
                if let Some(wt) = current.as_mut() {
                    wt.is_prunable = true;
                }
            }
            _ => {}
        }
    }
    if let Some(wt) = current {
        result.push(wt);
    }
    result
}

/// The common dir is `<main>/.git` for a normal repo; anything else (a bare
/// repo, a custom GIT_DIR) has no main checkout to point at, so keep toplevel.
fn main_worktree_root(common_dir: &Path, toplevel: &Path) -> PathBuf {
    match common_dir.parent() {
        Some(parent) if common_dir.file_name().is_some_and(|n| n == ".git") => parent.to_path_buf(),
        _ => toplevel.to_path_buf(),
    }
}

pub fn sanitize_branch_name(branch: &str) -> String {
    branch
        .trim()
        .replace(|c: char| c.is_whitespace(), "-")
        .replace(['/', '\\'], "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PORCELAIN: &str = "\
worktree /repo
HEAD aaaa1111
branch refs/heads/main

worktree /repo_feat
HEAD bbbb2222
branch refs/heads/feature/x
locked

worktree /repo_detached
HEAD cccc3333
detached
prunable gitdir file points to non-existent location
";

    #[test]
    fn parses_porcelain_output() {
        let wts = parse_worktree_list(PORCELAIN);
        assert_eq!(wts.len(), 3);

        assert_eq!(wts[0].path, "/repo");
        assert_eq!(wts[0].branch.as_deref(), Some("main"));
        assert_eq!(wts[0].head.as_deref(), Some("aaaa1111"));
        assert!(wts[0].is_base);

        // Branch names keep their slashes; only the refs/heads/ prefix is stripped.
        assert_eq!(wts[1].branch.as_deref(), Some("feature/x"));
        assert!(wts[1].is_locked);
        assert!(!wts[1].is_base);

        assert_eq!(wts[2].branch, None);
        assert_eq!(wts[2].detached, Some(true));
        assert!(wts[2].is_prunable);
    }

    #[test]
    fn parses_empty_output() {
        assert!(parse_worktree_list("").is_empty());
    }

    #[test]
    fn opens_linked_worktree_at_main_root() {
        let tmp = std::env::temp_dir().join(format!("wt-open-{}", std::process::id()));
        let main = tmp.join("app");
        let linked = tmp.join("app_feat");
        std::fs::create_dir_all(&main).unwrap();
        let git = |dir: &Path, args: &[&str]| {
            assert!(Command::new("git").args(args).current_dir(dir).status().unwrap().success());
        };
        git(&main, &["init", "-q"]);
        git(&main, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "--allow-empty", "-m", "i"]);
        git(&main, &["worktree", "add", "-q", "-b", "feat", linked.to_str().unwrap()]);

        let repo = GitRepo::open(&linked).unwrap();
        assert_eq!(repo.root.canonicalize().unwrap(), main.canonicalize().unwrap());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn sanitizes_branch_names() {
        assert_eq!(sanitize_branch_name(" feat/a b "), "feat-a-b");
        assert_eq!(sanitize_branch_name("plain"), "plain");
    }
}
