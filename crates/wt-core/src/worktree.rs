use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::config::load_config;
use crate::env::copy_env_files;
use crate::git::GitRepo;
use crate::hooks::{run_hooks, HookContext};
use crate::types::{AddOptions, PruneOptions, RemoveOptions, WorktreeInfo};

/// Add a new worktree. Returns the path of the created worktree.
pub fn add_worktree(repo: &GitRepo, opts: AddOptions) -> Result<PathBuf> {
    let base_top = repo.repo_root()?;
    let worktree_path = match &opts.dir {
        Some(dir) => dir.clone(),
        None => repo.default_worktree_dir(&opts.branch)?,
    };

    let cfg = load_config(&base_top)?;
    let add_hooks = cfg.hooks.as_ref().and_then(|h| h.add.as_ref());

    let hook_ctx = HookContext {
        base_top: &base_top,
        worktree_path: &worktree_path,
        branch: &opts.branch,
    };

    // Pre-create hooks
    if let Some(pre) = add_hooks.and_then(|h| h.pre_create.as_ref()) {
        run_hooks("hooks.add.pre_create", pre, &hook_ctx)?;
    }

    // Check target exists
    if worktree_path.exists() && !opts.force {
        return Err(anyhow!(
            "Target path already exists: {}\nUse --force to proceed.",
            worktree_path.display()
        ));
    }

    let wt_str = worktree_path.to_string_lossy().to_string();

    // Determine which git worktree add variant to use
    let local_exists = repo.branch_exists_local(&opts.branch);
    let remote_exists = repo.branch_exists_remote(&opts.remote, &opts.branch)?;

    if local_exists {
        repo.git_worktree_add(&[&wt_str, &opts.branch])?;
    } else if remote_exists && !opts.new_branch {
        let remote_ref = format!("{}/{}", opts.remote, opts.branch);
        repo.git_worktree_add(&["-b", &opts.branch, &wt_str, &remote_ref])?;
    } else {
        let base_ref = opts
            .base
            .unwrap_or_else(|| repo.current_branch().unwrap_or_else(|| "HEAD".to_string()));
        repo.git_worktree_add(&["-b", &opts.branch, &wt_str, &base_ref])?;
    }

    // Default post-create: copy env files
    let disable_default = add_hooks
        .and_then(|h| h.disable_default_post_create)
        .unwrap_or(false);
    if !disable_default {
        copy_env_files(&base_top, &worktree_path);
    }

    // Post-create hooks
    if let Some(post) = add_hooks.and_then(|h| h.post_create.as_ref()) {
        run_hooks("hooks.add.post_create", post, &hook_ctx)?;
    }

    Ok(worktree_path)
}

/// Remove a worktree. Returns the resolved path that was removed.
pub fn remove_worktree(repo: &GitRepo, opts: RemoveOptions) -> Result<PathBuf> {
    let resolved = if opts.as_path {
        let p = std::path::Path::new(&opts.target);
        Some(p.canonicalize().unwrap_or(p.to_path_buf()))
    } else {
        repo.resolve_worktree_path(&opts.target)?
    };

    let resolved = resolved.ok_or_else(|| {
        anyhow!("Cannot resolve worktree for: {}", opts.target)
    })?;

    let resolved_str = resolved.to_string_lossy().to_string();
    repo.git_worktree_remove(&resolved_str, opts.force)?;
    Ok(resolved)
}

/// List all worktrees.
pub fn list_worktrees(repo: &GitRepo) -> Result<Vec<WorktreeInfo>> {
    repo.list_worktrees()
}

/// Prune stale worktree info. Returns git output.
pub fn prune_worktrees(repo: &GitRepo, opts: PruneOptions) -> Result<String> {
    repo.git_worktree_prune(opts.dry_run, opts.verbose, opts.expire.as_deref())
}

/// Resolve a target to a worktree path (for switch command).
pub fn resolve_switch_target(repo: &GitRepo, target: &str) -> Result<PathBuf> {
    repo.resolve_worktree_path(target)?
        .ok_or_else(|| anyhow!("Cannot resolve worktree for: {}", target))
}
