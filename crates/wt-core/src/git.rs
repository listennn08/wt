// crates/wt-core/src/git.rs
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use git2::Repository;

use crate::types::WorktreeInfo;

pub struct GitRepo {
    repo: Repository,
}

impl GitRepo {
    pub fn open(path: &Path) -> Result<Self> {
        let repo = Repository::discover(path)
            .with_context(|| format!("Not a git repository: {}", path.display()))?;
        Ok(Self { repo })
    }

    pub fn repo_root(&self) -> Result<PathBuf> {
        let workdir = self
            .repo
            .workdir()
            .ok_or_else(|| anyhow!("Repository has no working directory"))?;
        Ok(workdir.to_path_buf())
    }

    pub fn repo_name(&self) -> Result<String> {
        let root = self.repo_root()?;
        root.file_name()
            .and_then(|n| n.to_str())
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("Cannot determine repository name"))
    }

    pub fn list_worktrees(&self) -> Result<Vec<WorktreeInfo>> {
        let base_path = self.repo_root()?;
        let base_str = base_path.to_string_lossy().to_string();
        let worktrees_names = self.repo.worktrees()?;

        let mut result = Vec::new();

        // Add base worktree
        let (branch, head) = self.get_head_info()?;
        result.push(WorktreeInfo {
            path: base_str.clone(),
            branch,
            head,
            is_base: true,
            is_locked: false,
            is_prunable: false,
            detached: None,
        });

        for name in worktrees_names.iter().flatten() {
            if let Ok(wt) = self.repo.find_worktree(name) {
                let wt_path = wt.path().to_string_lossy().to_string();
                let is_locked = matches!(
                    wt.is_locked(),
                    Ok(git2::WorktreeLockStatus::Locked(_))
                );
                let is_prunable = wt.is_prunable(None).unwrap_or(false);
                let (branch, head) = self.get_worktree_head_info(&wt);
                let is_base = wt_path == base_str;

                result.push(WorktreeInfo {
                    path: wt_path,
                    branch,
                    head,
                    is_base,
                    is_locked,
                    is_prunable,
                    detached: None,
                });
            }
        }

        result.sort_by(|a, b| {
            if a.is_base {
                std::cmp::Ordering::Less
            } else if b.is_base {
                std::cmp::Ordering::Greater
            } else {
                a.path.cmp(&b.path)
            }
        });

        Ok(result)
    }

    fn get_head_info(&self) -> Result<(Option<String>, Option<String>)> {
        let head = self.repo.head()?;
        let branch = if head.is_branch() {
            head.shorthand().map(|s| s.to_string())
        } else {
            None
        };
        let head_id = head
            .peel_to_commit()
            .ok()
            .map(|c| c.id().to_string());
        Ok((branch, head_id))
    }

    fn get_worktree_head_info(
        &self,
        worktree: &git2::Worktree,
    ) -> (Option<String>, Option<String>) {
        let wt_path = worktree.path();
        let wt_name = wt_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let main_path = self.repo.path().parent().unwrap_or(self.repo.path());
        let head_path = main_path
            .join(".git")
            .join("worktrees")
            .join(wt_name)
            .join("HEAD");

        let head_id = Repository::open(wt_path).ok().and_then(|r| {
            let head = r.head().ok()?;
            let commit = head.peel_to_commit().ok()?;
            Some(commit.id().to_string())
        });

        if let Ok(content) = std::fs::read_to_string(&head_path) {
            if let Some(branch) = content.trim().strip_prefix("ref: refs/heads/") {
                return (Some(branch.to_string()), head_id);
            }
        }

        (None, head_id)
    }

    pub fn branch_exists_local(&self, branch: &str) -> bool {
        self.repo.revparse_ext(branch).is_ok()
    }

    pub fn branch_exists_remote(&self, remote: &str, branch: &str) -> Result<bool> {
        let root = self.repo_root()?;
        let output = Command::new("git")
            .args(["ls-remote", "--heads", remote, branch])
            .current_dir(&root)
            .output()
            .context("Failed to run git ls-remote")?;
        Ok(!output.stdout.is_empty())
    }

    pub fn current_branch(&self) -> Option<String> {
        self.repo
            .head()
            .ok()
            .filter(|h| h.is_branch())
            .and_then(|h| h.shorthand().map(|s| s.to_string()))
    }

    pub fn default_worktree_dir(&self, branch: &str) -> Result<PathBuf> {
        let root = self.repo_root()?;
        let parent = root
            .parent()
            .ok_or_else(|| anyhow!("Cannot determine repository parent directory"))?;
        let name = self.repo_name()?;
        let sanitized = sanitize_branch_name(branch);
        Ok(parent.join(format!("{}_{}", name, sanitized)))
    }

    pub fn git_worktree_add(&self, args: &[&str]) -> Result<()> {
        let root = self.repo_root()?;
        let status = Command::new("git")
            .arg("worktree")
            .arg("add")
            .args(args)
            .current_dir(&root)
            .status()
            .context("Failed to run git worktree add")?;
        if !status.success() {
            return Err(anyhow!("git worktree add failed"));
        }
        Ok(())
    }

    pub fn git_worktree_remove(&self, path: &str, force: bool) -> Result<()> {
        let root = self.repo_root()?;
        let mut cmd = Command::new("git");
        cmd.args(["worktree", "remove"]);
        if force {
            cmd.arg("--force");
        }
        cmd.arg(path);
        let status = cmd
            .current_dir(&root)
            .status()
            .context("Failed to run git worktree remove")?;
        if !status.success() {
            return Err(anyhow!("git worktree remove failed"));
        }
        Ok(())
    }

    pub fn git_worktree_prune(
        &self,
        dry_run: bool,
        verbose: bool,
        expire: Option<&str>,
    ) -> Result<String> {
        let root = self.repo_root()?;
        let mut cmd = Command::new("git");
        cmd.args(["worktree", "prune"]);
        if dry_run {
            cmd.arg("--dry-run");
        }
        if verbose {
            cmd.arg("--verbose");
        }
        if let Some(expire) = expire {
            cmd.arg(format!("--expire={}", expire));
        }
        let output = cmd
            .current_dir(&root)
            .output()
            .context("Failed to run git worktree prune")?;
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub fn list_branches(&self) -> Result<Vec<String>> {
        let root = self.repo_root()?;
        let output = Command::new("git")
            .args([
                "for-each-ref",
                "--format=%(refname:short)",
                "refs/heads",
                "refs/remotes",
            ])
            .current_dir(&root)
            .output()
            .context("Failed to list branches")?;
        let text = String::from_utf8_lossy(&output.stdout);
        let mut branches: Vec<String> = text
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|b| !b.is_empty() && !b.ends_with("/HEAD"))
            .collect();
        branches.sort();
        branches.dedup();
        Ok(branches)
    }

    pub fn list_worktree_paths(&self) -> Result<Vec<String>> {
        let worktrees = self.list_worktrees()?;
        Ok(worktrees
            .into_iter()
            .filter(|wt| !wt.is_base)
            .map(|wt| wt.path)
            .collect())
    }

    pub fn resolve_worktree_path(&self, target: &str) -> Result<Option<PathBuf>> {
        let as_path = Path::new(target);
        if as_path.exists() {
            return Ok(Some(as_path.canonicalize().unwrap_or(as_path.to_path_buf())));
        }

        let worktrees = self.list_worktrees()?;
        for wt in &worktrees {
            if let Some(branch) = &wt.branch {
                if branch == target {
                    return Ok(Some(PathBuf::from(&wt.path)));
                }
            }
        }

        let fallback = self.default_worktree_dir(target)?;
        if fallback.exists() {
            return Ok(Some(fallback));
        }

        Ok(None)
    }
}

pub fn sanitize_branch_name(branch: &str) -> String {
    branch
        .trim()
        .replace(|c: char| c.is_whitespace(), "-")
        .replace(['/', '\\'], "-")
}
