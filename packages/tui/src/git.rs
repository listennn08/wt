use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{anyhow, Result};
use git2::{Repository, Worktree};

use crate::app::Worktree as AppWorktree;

pub struct GitRepo {
    repo: Repository,
}

impl GitRepo {
    pub fn new(path: &str) -> Result<Self> {
        let repo = Repository::open(path)?;
        Ok(Self { repo })
    }

    pub fn get_worktrees(&self) -> Result<Vec<AppWorktree>> {
        let worktrees = self.repo.worktrees()?;
        let base_path = self.get_base_path()?;

        let mut app_worktrees = Vec::new();

        // Add the base/main repository as a worktree
        {
            let (branch, head) = self.get_base_info()?;
            let base_worktree = AppWorktree {
                path: base_path.clone(),
                branch,
                head,
                is_base: true,
                is_locked: false, // Base repo is never locked
                is_prunable: false, // Base repo cannot be pruned
            };
            app_worktrees.push(base_worktree);
        }

        for worktree_name in worktrees.iter().flatten() {
            if let Ok(worktree) = self.repo.find_worktree(worktree_name) {
                let path = worktree.path().to_string_lossy().to_string();
                let is_base = path == base_path;

                // Try to get branch and head info
                let (branch, head) = self.get_worktree_info(&worktree)?;

                let app_worktree = AppWorktree {
                    path,
                    branch,
                    head,
                    is_base,
                    is_locked: matches!(worktree.is_locked()?, git2::WorktreeLockStatus::Locked(_)),
                    is_prunable: worktree.is_prunable(None)?,
                };

                app_worktrees.push(app_worktree);
            }
        }

        // Sort worktrees, with base first
        app_worktrees.sort_by(|a, b| {
            if a.is_base {
                std::cmp::Ordering::Less
            } else if b.is_base {
                std::cmp::Ordering::Greater
            } else {
                a.path.cmp(&b.path)
            }
        });

        Ok(app_worktrees)
    }

    pub fn get_base_path(&self) -> Result<String> {
        let workdir = self.repo.workdir()
            .ok_or_else(|| anyhow!("Repository has no working directory"))?;
        Ok(workdir.to_string_lossy().to_string())
    }

    pub fn remove_worktree(&self, path: &str) -> Result<()> {
        let normalized = Path::new(path).canonicalize().unwrap_or_else(|_| Path::new(path).to_path_buf());
        let binding = self.get_base_path()?;
        let base = Path::new(&binding);
        if normalized == base {
            return Err(anyhow!("Cannot remove the base repository worktree"));
        }

        // Find the worktree by path
        let worktrees = self.repo.worktrees()?;
        for worktree_name in worktrees.iter().flatten() {
            if let Ok(worktree) = self.repo.find_worktree(worktree_name) {
                if worktree.path().to_string_lossy() == path {
                    worktree.prune(Some(git2::WorktreePruneOptions::new().valid(true)))?;

                    let fs_path = Path::new(path);
                    if fs_path.exists() {
                        std::fs::remove_dir_all(fs_path)?;
                    }

                    return Ok(());
                }
            }
        }
        Err(anyhow!("Worktree not found: {}", path))
    }

    pub fn prune_worktrees(&self) -> Result<()> {
        let mut opts = git2::WorktreePruneOptions::new();
        opts.valid(true);

        let worktrees = self.repo.worktrees()?;
        for worktree_name in worktrees.iter().flatten() {
            if let Ok(worktree) = self.repo.find_worktree(worktree_name) {
                if worktree.is_prunable(Some(&mut opts))? {
                    worktree.prune(Some(&mut opts))?;
                }
            }
        }
        Ok(())
    }

    pub fn add_worktree_from_branch(&self, branch: &str) -> Result<String> {
        let base = self.get_base_path()?;
        let branch = branch.trim();
        if branch.is_empty() {
            return Err(anyhow!("Branch name is required"));
        }

        let target = self.default_worktree_dir(&base, branch)?;

        if target.exists() {
            return Err(anyhow!(
                "Target path already exists: {}",
                target.to_string_lossy()
            ));
        }

        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let target_str = target.to_string_lossy().to_string();
        let mut args = vec!["worktree".to_string(), "add".to_string()];

        if self.branch_exists_local(branch)? {
            args.push(target_str.clone());
            args.push(branch.to_string());
        } else if self.branch_exists_remote("origin", branch)? {
            args.push("-b".to_string());
            args.push(branch.to_string());
            args.push(target_str.clone());
            args.push(format!("origin/{}", branch));
        } else {
            let base_ref = self.current_branch_ref().unwrap_or_else(|| "HEAD".to_string());
            args.push("-b".to_string());
            args.push(branch.to_string());
            args.push(target_str.clone());
            args.push(base_ref);
        }

        let status = Command::new("git")
            .args(args.iter().map(|s| s.as_str()))
            .current_dir(&base)
            .status()?;

        if !status.success() {
            return Err(anyhow!(format!(
                "Failed to create worktree for branch {}",
                branch
            )));
        }

        Ok(target_str)
    }

    fn default_worktree_dir(&self, base_path: &str, branch: &str) -> Result<PathBuf> {
        let top = Path::new(base_path);
        let parent = top
            .parent()
            .ok_or_else(|| anyhow!("Cannot determine repository parent directory"))?;
        let repo_name = top
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow!("Cannot determine repository name"))?;
        let sanitized = sanitize_branch_name(branch);
        Ok(parent.join(format!("{}_{}", repo_name, sanitized)))
    }

    fn branch_exists_local(&self, branch: &str) -> Result<bool> {
        Ok(self.repo.revparse_ext(branch).is_ok())
    }

    fn branch_exists_remote(&self, remote: &str, branch: &str) -> Result<bool> {
        let base = self.get_base_path()?;
        let status = Command::new("git")
            .args(["ls-remote", "--heads", remote, branch])
            .current_dir(base)
            .output()?;
        Ok(!status.stdout.is_empty())
    }

    fn current_branch_ref(&self) -> Option<String> {
        if let Ok(head) = self.repo.head() {
            if head.is_branch() {
                return head.shorthand().map(|s| s.to_string());
            }
        }
        None
    }

    fn get_base_info(&self) -> Result<(Option<String>, Option<String>)> {
        // Get current branch
        let head = self.repo.head()?;
        let branch = if head.is_branch() {
            head.shorthand().map(|s| s.to_string())
        } else {
            None
        };

        // Get HEAD commit
        let head_commit = head.peel_to_commit();
        let head_id = head_commit.ok().map(|commit| commit.id().to_string());

        Ok((branch, head_id))
    }

    fn get_worktree_info(&self, worktree: &Worktree) -> Result<(Option<String>, Option<String>)> {
        // For worktrees, HEAD is in .git/worktrees/<name>/HEAD, not in the worktree directory
        // We need to construct the path manually
        let worktree_path = worktree.path();
        let worktree_name = worktree_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let main_repo_path = self.repo.path().parent().unwrap_or(self.repo.path());
        let head_path = main_repo_path.join(".git").join("worktrees").join(worktree_name).join("HEAD");

        if head_path.exists() {
            if let Ok(head_content) = std::fs::read_to_string(&head_path) {
                if head_content.starts_with("ref: refs/heads/") {
                    let branch = head_content.trim()
                        .strip_prefix("ref: refs/heads/")
                        .unwrap_or("")
                        .to_string();

                    // Also try to get the commit hash that this branch points to
                    let worktree_repo = worktree_to_repo(worktree)?;
                    let head_commit = worktree_repo.head()?.peel_to_commit();
                    let head = head_commit.ok().map(|commit| commit.id().to_string());

                    return Ok((Some(branch), head));
                }
            }
        }

        // Try to get HEAD commit if not on a branch
        let binding = worktree_to_repo(worktree)?;
        let head_commit = binding.head()?.peel_to_commit();
        if let Ok(commit) = head_commit {
            let head = Some(commit.id().to_string());
            Ok((None, head))
        } else {
            Ok((None, None))
        }
    }
}

fn worktree_to_repo(worktree: &Worktree) -> Result<Repository> {
    // For worktrees, we can open the repository directly from the worktree path
    Ok(Repository::open(worktree.path())?)
}

fn sanitize_branch_name(branch: &str) -> String {
    branch
        .trim()
        .replace(|c: char| c.is_whitespace(), "-")
        .replace(['/', '\\'], "-")
}
