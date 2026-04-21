# wt Rust Rewrite Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite the `wt` git worktree manager CLI from TypeScript to Rust as a single binary, keeping TUI intact and removing the MCP server.

**Architecture:** Cargo workspace with `wt-core` lib crate (git/config/hooks/env logic), `wt-tui` lib crate (Ratatui TUI), and a root bin crate that routes CLI commands via clap. The TUI code migrates from `packages/tui/` with refactoring to use `wt-core` instead of its own git logic.

**Tech Stack:** Rust (2021 edition), clap 4 (derive), git2 0.18, toml + serde, colored, ratatui 0.26, crossterm 0.27, portable-pty 0.9, tokio (TUI only), anyhow + thiserror

---

## File Structure

### New files to create

```
Cargo.toml                          # workspace root + bin crate
crates/wt-core/Cargo.toml
crates/wt-core/src/lib.rs           # re-exports
crates/wt-core/src/git.rs           # git2 operations
crates/wt-core/src/config.rs        # TOML config parsing
crates/wt-core/src/hooks.rs         # hook execution
crates/wt-core/src/env.rs           # .env copying
crates/wt-core/src/worktree.rs      # orchestration (add/remove/list/prune/switch)
crates/wt-core/src/types.rs         # shared types (WorktreeInfo, AddOptions, etc.)
crates/wt-tui/Cargo.toml
crates/wt-tui/src/lib.rs
crates/wt-tui/src/app.rs            # migrated from packages/tui/src/app.rs
crates/wt-tui/src/ui.rs             # migrated from packages/tui/src/ui.rs
crates/wt-tui/src/terminal.rs       # migrated from packages/tui/src/terminal.rs
src/main.rs                         # clap CLI definition + routing
src/cmd/mod.rs
src/cmd/add.rs
src/cmd/list.rs
src/cmd/remove.rs
src/cmd/switch.rs
src/cmd/tui.rs
src/cmd/prune.rs
src/cmd/completion.rs
src/cmd/uninstall.rs
src/output.rs                       # [wt] prefix + colored output helpers
```

### Files to delete (Phase 6)

```
packages/core/                      # entire TypeScript CLI
packages/mcp/                       # MCP server
packages/tui/                       # old TUI location (migrated to crates/)
pnpm-workspace.yaml
tsconfig.json
```

---

### Task 1: Cargo Workspace + wt-core Types and Config

**Files:**
- Create: `Cargo.toml`
- Create: `crates/wt-core/Cargo.toml`
- Create: `crates/wt-core/src/lib.rs`
- Create: `crates/wt-core/src/types.rs`
- Create: `crates/wt-core/src/config.rs`

- [ ] **Step 1: Create workspace root Cargo.toml**

```toml
[workspace]
members = ["crates/wt-core", "crates/wt-tui"]
resolver = "2"

[package]
name = "wt-cli"
version = "1.0.0"
edition = "2021"
default-run = "wt"

[[bin]]
name = "wt"
path = "src/main.rs"

[dependencies]
wt-core = { path = "crates/wt-core" }
wt-tui = { path = "crates/wt-tui" }
clap = { version = "4", features = ["derive"] }
clap_complete = "4"
colored = "2"
anyhow = "1"
serde_json = "1"
tokio = { version = "1", features = ["full"] }
```

- [ ] **Step 2: Create wt-core Cargo.toml**

```toml
[package]
name = "wt-core"
version = "1.0.0"
edition = "2021"

[dependencies]
git2 = "0.18"
toml = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
anyhow = "1"
thiserror = "1"
dirs = "5"
```

- [ ] **Step 3: Create types.rs with shared types**

```rust
// crates/wt-core/src/types.rs
use std::path::PathBuf;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WorktreeInfo {
    pub path: String,
    pub branch: Option<String>,
    pub head: Option<String>,
    pub is_base: bool,
    pub is_locked: bool,
    pub is_prunable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detached: Option<bool>,
}

#[derive(Debug)]
pub struct AddOptions {
    pub branch: String,
    pub dir: Option<PathBuf>,
    pub new_branch: bool,
    pub base: Option<String>,
    pub remote: String,
    pub force: bool,
    pub progress: bool,
}

impl Default for AddOptions {
    fn default() -> Self {
        Self {
            branch: String::new(),
            dir: None,
            new_branch: false,
            base: None,
            remote: "origin".to_string(),
            force: false,
            progress: true,
        }
    }
}

#[derive(Debug)]
pub struct PruneOptions {
    pub dry_run: bool,
    pub verbose: bool,
    pub expire: Option<String>,
}

#[derive(Debug)]
pub struct RemoveOptions {
    pub target: String,
    pub force: bool,
    pub as_branch: bool,
    pub as_path: bool,
}
```

- [ ] **Step 4: Create config.rs with TOML config parsing**

```rust
// crates/wt-core/src/config.rs
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct WtConfig {
    pub hooks: Option<HooksConfig>,
}

#[derive(Debug, Deserialize)]
pub struct HooksConfig {
    pub add: Option<AddHooks>,
}

#[derive(Debug, Deserialize)]
pub struct AddHooks {
    pub pre_create: Option<Vec<HookCommand>>,
    pub post_create: Option<Vec<HookCommand>>,
    pub disable_default_post_create: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HookCommand {
    pub program: String,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
}

fn config_candidates(repo_root: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![
        repo_root.join(".wt.toml"),
        repo_root.join("wt.toml"),
        repo_root.join(".config").join("wt").join("config.toml"),
    ];

    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        candidates.push(PathBuf::from(xdg).join("wt").join("config.toml"));
    } else if let Some(home) = dirs::home_dir() {
        candidates.push(home.join(".config").join("wt").join("config.toml"));
    }

    candidates
}

pub fn load_config(repo_root: &Path) -> Result<WtConfig> {
    for candidate in config_candidates(repo_root) {
        if candidate.exists() {
            let content = fs::read_to_string(&candidate)
                .with_context(|| format!("Failed to read config: {}", candidate.display()))?;
            let config: WtConfig = toml::from_str(&content)
                .with_context(|| format!("Failed to parse config: {}", candidate.display()))?;
            return Ok(config);
        }
    }
    Ok(WtConfig::default())
}
```

- [ ] **Step 5: Create lib.rs with re-exports**

```rust
// crates/wt-core/src/lib.rs
pub mod config;
pub mod types;
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check -p wt-core`
Expected: compiles with no errors

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/
git commit -m "feat: scaffold Cargo workspace with wt-core types and config"
```

---

### Task 2: wt-core Git Module

**Files:**
- Create: `crates/wt-core/src/git.rs`
- Modify: `crates/wt-core/src/lib.rs`

- [ ] **Step 1: Create git.rs — GitRepo struct and list_worktrees**

This is migrated and expanded from `packages/tui/src/git.rs`.

```rust
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

        // Sort: base first, then by path
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

        if let Ok(content) = std::fs::read_to_string(&head_path) {
            if let Some(branch) = content.trim().strip_prefix("ref: refs/heads/") {
                let head_id = Repository::open(wt_path)
                    .ok()
                    .and_then(|r| r.head().ok())
                    .and_then(|h| h.peel_to_commit().ok())
                    .map(|c| c.id().to_string());
                return (Some(branch.to_string()), head_id);
            }
        }

        let head_id = Repository::open(wt_path)
            .ok()
            .and_then(|r| r.head().ok())
            .and_then(|h| h.peel_to_commit().ok())
            .map(|c| c.id().to_string());
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

    /// Run `git worktree add` with the appropriate arguments.
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

    /// Run `git worktree remove`.
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

    /// Run `git worktree prune`.
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

    /// List all branch names (local + remote) for shell completion.
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

    /// List worktree paths (excluding base) for shell completion.
    pub fn list_worktree_paths(&self) -> Result<Vec<String>> {
        let worktrees = self.list_worktrees()?;
        Ok(worktrees
            .into_iter()
            .filter(|wt| !wt.is_base)
            .map(|wt| wt.path)
            .collect())
    }

    /// Resolve a target (path or branch name) to a worktree path.
    pub fn resolve_worktree_path(&self, target: &str) -> Result<Option<PathBuf>> {
        let as_path = Path::new(target);
        if as_path.exists() {
            return Ok(Some(as_path.canonicalize().unwrap_or(as_path.to_path_buf())));
        }

        // Try to find by branch name
        let worktrees = self.list_worktrees()?;
        for wt in &worktrees {
            if let Some(branch) = &wt.branch {
                if branch == target {
                    return Ok(Some(PathBuf::from(&wt.path)));
                }
            }
        }

        // Fallback: check default worktree dir
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
```

- [ ] **Step 2: Add git module to lib.rs**

```rust
// crates/wt-core/src/lib.rs
pub mod config;
pub mod git;
pub mod types;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p wt-core`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add crates/wt-core/src/git.rs crates/wt-core/src/lib.rs
git commit -m "feat(wt-core): add git module with worktree operations"
```

---

### Task 3: wt-core Hooks and Env Modules

**Files:**
- Create: `crates/wt-core/src/hooks.rs`
- Create: `crates/wt-core/src/env.rs`
- Modify: `crates/wt-core/src/lib.rs`

- [ ] **Step 1: Create hooks.rs**

```rust
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
```

- [ ] **Step 2: Create env.rs**

```rust
// crates/wt-core/src/env.rs
use std::fs;
use std::path::Path;

const ENV_FILES: &[&str] = &[".env", ".env.local"];

/// Copy .env files from base worktree to new worktree (skip if destination exists).
pub fn copy_env_files(base: &Path, worktree: &Path) -> Vec<String> {
    let mut copied = Vec::new();
    for filename in ENV_FILES {
        let src = base.join(filename);
        let dst = worktree.join(filename);
        if !src.exists() || dst.exists() {
            continue;
        }
        if fs::copy(&src, &dst).is_ok() {
            copied.push(filename.to_string());
        }
    }
    copied
}
```

- [ ] **Step 3: Update lib.rs**

```rust
// crates/wt-core/src/lib.rs
pub mod config;
pub mod env;
pub mod git;
pub mod hooks;
pub mod types;
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check -p wt-core`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add crates/wt-core/src/hooks.rs crates/wt-core/src/env.rs crates/wt-core/src/lib.rs
git commit -m "feat(wt-core): add hooks execution and env file copying"
```

---

### Task 4: wt-core Worktree Orchestration

**Files:**
- Create: `crates/wt-core/src/worktree.rs`
- Modify: `crates/wt-core/src/lib.rs`

- [ ] **Step 1: Create worktree.rs — orchestration layer**

```rust
// crates/wt-core/src/worktree.rs
use std::path::{Path, PathBuf};

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
        let p = Path::new(&opts.target);
        Some(p.canonicalize().unwrap_or(p.to_path_buf()))
    } else if opts.as_branch {
        repo.resolve_worktree_path(&opts.target)?
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
```

- [ ] **Step 2: Update lib.rs**

```rust
// crates/wt-core/src/lib.rs
pub mod config;
pub mod env;
pub mod git;
pub mod hooks;
pub mod types;
pub mod worktree;
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check -p wt-core`
Expected: compiles with no errors

- [ ] **Step 4: Commit**

```bash
git add crates/wt-core/src/worktree.rs crates/wt-core/src/lib.rs
git commit -m "feat(wt-core): add worktree orchestration (add/remove/list/prune/switch)"
```

---

### Task 5: Bin Crate — CLI Skeleton + List Command

**Files:**
- Create: `src/main.rs`
- Create: `src/output.rs`
- Create: `src/cmd/mod.rs`
- Create: `src/cmd/list.rs`

- [ ] **Step 1: Create output.rs — colored output helper**

```rust
// src/output.rs
use colored::Colorize;

pub fn log(msg: &str) {
    println!("{} {}", "[wt]".on_green().white(), msg);
}
```

- [ ] **Step 2: Create cmd/list.rs**

```rust
// src/cmd/list.rs
use anyhow::Result;
use clap::Args;
use colored::Colorize;
use wt_core::git::GitRepo;
use wt_core::types::WorktreeInfo;
use wt_core::worktree;

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Print raw `git worktree list` output
    #[arg(long)]
    raw: bool,

    /// Print JSON output
    #[arg(long)]
    json: bool,
}

pub fn run(args: ListArgs) -> Result<()> {
    let repo = GitRepo::open(&std::env::current_dir()?)?;

    if args.raw {
        let root = repo.repo_root()?;
        let output = std::process::Command::new("git")
            .args(["worktree", "list"])
            .current_dir(&root)
            .output()?;
        print!("{}", String::from_utf8_lossy(&output.stdout));
        return Ok(());
    }

    let worktrees = worktree::list_worktrees(&repo)?;

    if args.json {
        let json = serde_json::to_string_pretty(&worktrees)?;
        println!("{}", json);
        return Ok(());
    }

    print_table(&worktrees);
    Ok(())
}

fn pad(s: &str, width: usize) -> String {
    if s.len() >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - s.len()))
    }
}

fn print_table(worktrees: &[WorktreeInfo]) {
    let rows: Vec<_> = worktrees
        .iter()
        .map(|wt| {
            let branch = wt.branch.as_deref().unwrap_or(
                if wt.detached == Some(true) { "detached" } else { "" }
            );
            let head = wt
                .head
                .as_deref()
                .map(|h| if h.len() > 8 { &h[..8] } else { h })
                .unwrap_or("");
            let mut flags = Vec::new();
            if wt.is_base { flags.push("base"); }
            if wt.is_locked { flags.push("locked"); }
            if wt.is_prunable { flags.push("prunable"); }
            (wt.path.as_str(), branch, head, flags, wt.is_base)
        })
        .collect();

    let path_width = rows.iter().map(|r| r.0.len()).max().unwrap_or(4).min(60);
    let branch_width = rows.iter().map(|r| r.1.len()).max().unwrap_or(6).min(30);

    // Header
    let header = format!(
        "{}  {}  {}  FLAGS",
        pad("BRANCH", branch_width),
        pad("PATH", path_width),
        pad("HEAD", 8),
    );
    println!("{}", header.bold().dimmed());

    for (path, branch, head, flags, is_base) in &rows {
        let p = if path.len() > path_width {
            format!("…{}", &path[path.len() - (path_width - 1)..])
        } else {
            path.to_string()
        };

        let branch_col = if branch.is_empty() {
            pad("", *&branch_width)
        } else {
            format!("{}", pad(branch, branch_width).cyan())
        };
        let path_col = format!("{}", pad(&p, path_width).dimmed());
        let head_col = if head.is_empty() {
            pad("", 8)
        } else {
            format!("{}", pad(head, 8).dimmed())
        };
        let flags_col: String = flags
            .iter()
            .map(|f| match *f {
                "base" => "base".green().to_string(),
                "locked" => "locked".yellow().to_string(),
                "prunable" => "prunable".red().to_string(),
                other => other.dimmed().to_string(),
            })
            .collect::<Vec<_>>()
            .join(&",".dimmed().to_string());

        let line = format!("{}  {}  {}  {}", branch_col, path_col, head_col, flags_col);
        if *is_base {
            println!("{}", line.bold());
        } else {
            println!("{}", line);
        }
    }
}
```

- [ ] **Step 3: Create cmd/mod.rs**

```rust
// src/cmd/mod.rs
pub mod list;
```

- [ ] **Step 4: Create main.rs with initial CLI skeleton**

```rust
// src/main.rs
mod cmd;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "wt", about = "Git worktree manager", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all worktrees
    List(cmd::list::ListArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::List(args) => cmd::list::run(args),
    }
}
```

- [ ] **Step 5: Verify it compiles and runs**

Run: `cargo run -- list`
Expected: prints worktree table with colored output

Run: `cargo run -- list --json`
Expected: prints JSON array of worktrees

- [ ] **Step 6: Commit**

```bash
git add src/ crates/
git commit -m "feat: add CLI skeleton with list command"
```

---

### Task 6: Add Command

**Files:**
- Create: `src/cmd/add.rs`
- Modify: `src/cmd/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create cmd/add.rs**

```rust
// src/cmd/add.rs
use std::path::PathBuf;

use anyhow::Result;
use clap::Args;
use wt_core::git::GitRepo;
use wt_core::types::AddOptions;
use wt_core::worktree;

use crate::output;

#[derive(Args, Debug)]
pub struct AddArgs {
    /// Branch to create/use for worktree
    branch: String,

    /// Target directory for the worktree
    #[arg(short, long)]
    dir: Option<PathBuf>,

    /// Force creating a new branch even if remote branch exists
    #[arg(short = 'n', long)]
    new_branch: bool,

    /// Base ref when creating a new branch (default: current branch)
    #[arg(short, long)]
    base: Option<String>,

    /// Remote name (default: origin)
    #[arg(short, long, default_value = "origin")]
    remote: String,

    /// Allow if target directory already exists
    #[arg(short, long)]
    force: bool,

    /// Do not print step/progress messages
    #[arg(long)]
    no_progress: bool,
}

pub fn run(args: AddArgs) -> Result<()> {
    let repo = GitRepo::open(&std::env::current_dir()?)?;
    let progress = !args.no_progress;

    let opts = AddOptions {
        branch: args.branch,
        dir: args.dir,
        new_branch: args.new_branch,
        base: args.base,
        remote: args.remote,
        force: args.force,
        progress,
    };

    let path = worktree::add_worktree(&repo, opts)?;
    if progress {
        output::log(&path.to_string_lossy());
    } else {
        println!("{}", path.display());
    }
    Ok(())
}
```

- [ ] **Step 2: Update cmd/mod.rs**

```rust
// src/cmd/mod.rs
pub mod add;
pub mod list;
```

- [ ] **Step 3: Update main.rs**

```rust
// src/main.rs
mod cmd;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "wt", about = "Git worktree manager", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new worktree from a branch
    Add(cmd::add::AddArgs),
    /// List all worktrees
    List(cmd::list::ListArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Add(args) => cmd::add::run(args),
        Commands::List(args) => cmd::list::run(args),
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 5: Commit**

```bash
git add src/cmd/add.rs src/cmd/mod.rs src/main.rs
git commit -m "feat: add 'wt add' command"
```

---

### Task 7: Remove, Switch, Prune, Uninstall Commands

**Files:**
- Create: `src/cmd/remove.rs`
- Create: `src/cmd/switch.rs`
- Create: `src/cmd/prune.rs`
- Create: `src/cmd/uninstall.rs`
- Modify: `src/cmd/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create cmd/remove.rs**

```rust
// src/cmd/remove.rs
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
```

- [ ] **Step 2: Create cmd/switch.rs**

```rust
// src/cmd/switch.rs
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
    } else if args.branch {
        worktree::resolve_switch_target(&repo, &args.target)?
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
```

- [ ] **Step 3: Create cmd/prune.rs**

```rust
// src/cmd/prune.rs
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
```

- [ ] **Step 4: Create cmd/uninstall.rs**

```rust
// src/cmd/uninstall.rs
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Result;
use clap::Args;

use crate::output;

#[derive(Args, Debug)]
pub struct UninstallArgs {
    /// Shell name (zsh|fish|bash|all)
    #[arg(long)]
    shell: Option<String>,

    /// Do not prompt
    #[arg(long)]
    yes: bool,
}

pub fn run(args: UninstallArgs) -> Result<()> {
    let shell = args
        .shell
        .unwrap_or_else(|| detect_shell())
        .to_lowercase();

    match shell.as_str() {
        "zsh" => uninstall_zsh_completion(),
        "fish" => uninstall_fish_completion(),
        "bash" => uninstall_bash_completion(),
        "all" => {
            uninstall_zsh_completion();
            uninstall_fish_completion();
            uninstall_bash_completion();
        }
        _ => {
            eprintln!("Only zsh, fish, bash, and all are supported");
            std::process::exit(1);
        }
    }

    if let Some(binary) = resolve_wt_binary() {
        println!("wt binary:\n- {}", binary);
    }

    println!("Uninstall package (if installed globally):");
    println!("- cargo uninstall wt-cli");
    Ok(())
}

fn detect_shell() -> String {
    if std::env::var("ZSH_VERSION").is_ok() { return "zsh".into(); }
    if std::env::var("FISH_VERSION").is_ok() { return "fish".into(); }
    if std::env::var("BASH_VERSION").is_ok() { return "bash".into(); }
    std::env::var("SHELL")
        .ok()
        .and_then(|s| {
            std::path::Path::new(&s)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "zsh".into())
}

fn resolve_wt_binary() -> Option<String> {
    Command::new("which")
        .arg("wt")
        .output()
        .ok()
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        })
}

fn uninstall_zsh_completion() {
    let home = dirs::home_dir().unwrap();
    let completion_file = home.join(".zsh").join("completions").join("_wt");
    let zshrc = home.join(".zshrc");

    if completion_file.exists() {
        if fs::remove_file(&completion_file).is_ok() {
            output::log(&format!("removed {}", completion_file.display()));
        }
    }

    remove_block_from_file(&zshrc, "# wt completion start", "# wt completion end");
}

fn uninstall_fish_completion() {
    let home = dirs::home_dir().unwrap();
    let completion_file = home
        .join(".config")
        .join("fish")
        .join("completions")
        .join("wt.fish");

    if completion_file.exists() {
        if fs::remove_file(&completion_file).is_ok() {
            output::log(&format!("removed {}", completion_file.display()));
        }
    }
}

fn uninstall_bash_completion() {
    let home = dirs::home_dir().unwrap();
    let completion_file = home.join(".bash_completion.d").join("wt");
    let bashrc = home.join(".bashrc");

    if completion_file.exists() {
        if fs::remove_file(&completion_file).is_ok() {
            output::log(&format!("removed {}", completion_file.display()));
        }
    }

    remove_block_from_file(&bashrc, "# wt completion start", "# wt completion end");
}

fn remove_block_from_file(path: &PathBuf, start_marker: &str, end_marker: &str) {
    if !path.exists() { return; }
    let Ok(content) = fs::read_to_string(path) else { return; };
    let start = content.find(start_marker);
    let end = content.find(end_marker);
    if let (Some(s), Some(e)) = (start, end) {
        if e < s { return; }
        let cut_end = content[e..].find('\n').map_or(content.len(), |i| e + i + 1);
        let next = format!("{}{}", &content[..s], &content[cut_end..])
            .replace("\n\n\n", "\n\n");
        if fs::write(path, &next).is_ok() {
            output::log(&format!("updated {}", path.display()));
        }
    }
}
```

- [ ] **Step 5: Update cmd/mod.rs**

```rust
// src/cmd/mod.rs
pub mod add;
pub mod list;
pub mod prune;
pub mod remove;
pub mod switch;
pub mod uninstall;
```

- [ ] **Step 6: Update main.rs with all commands**

```rust
// src/main.rs
mod cmd;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "wt", about = "Git worktree manager", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new worktree from a branch
    Add(cmd::add::AddArgs),
    /// List all worktrees
    List(cmd::list::ListArgs),
    /// Delete a worktree
    #[command(alias = "rm")]
    Remove(cmd::remove::RemoveArgs),
    /// Switch to a worktree and open a shell in its directory
    #[command(alias = "sw")]
    Switch(cmd::switch::SwitchArgs),
    /// Prune stale worktree information
    Prune(cmd::prune::PruneArgs),
    /// Remove wt shell completions and print uninstall instructions
    Uninstall(cmd::uninstall::UninstallArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Add(args) => cmd::add::run(args),
        Commands::List(args) => cmd::list::run(args),
        Commands::Remove(args) => cmd::remove::run(args),
        Commands::Switch(args) => cmd::switch::run(args),
        Commands::Prune(args) => cmd::prune::run(args),
        Commands::Uninstall(args) => cmd::uninstall::run(args),
    }
}
```

- [ ] **Step 7: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 8: Commit**

```bash
git add src/
git commit -m "feat: add remove, switch, prune, uninstall commands"
```

---

### Task 8: TUI Migration

**Files:**
- Create: `crates/wt-tui/Cargo.toml`
- Create: `crates/wt-tui/src/lib.rs`
- Create: `crates/wt-tui/src/app.rs`
- Create: `crates/wt-tui/src/ui.rs`
- Create: `crates/wt-tui/src/terminal.rs`
- Create: `src/cmd/tui.rs`
- Modify: `src/cmd/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create crates/wt-tui/Cargo.toml**

```toml
[package]
name = "wt-tui"
version = "1.0.0"
edition = "2021"

[dependencies]
wt-core = { path = "../wt-core" }
ratatui = "0.26"
crossterm = "0.27"
tokio = { version = "1", features = ["full"] }
portable-pty = "0.9"
vt100 = "0.15"
anyhow = "1"
```

- [ ] **Step 2: Copy terminal.rs from existing TUI (unchanged)**

Copy `packages/tui/src/terminal.rs` to `crates/wt-tui/src/terminal.rs`. This file has no dependencies on the old git module so it needs no changes.

- [ ] **Step 3: Create crates/wt-tui/src/app.rs — refactored to use wt-core**

The key change: replace `crate::git::GitRepo` with `wt_core::git::GitRepo` and `crate::app::Worktree` with `wt_core::types::WorktreeInfo`. The `App` struct uses `wt_core` for all git operations.

```rust
// crates/wt-tui/src/app.rs
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::backend::Backend;
use ratatui::Terminal;

use wt_core::git::GitRepo;
use wt_core::types::{AddOptions, WorktreeInfo};
use wt_core::worktree;

use crate::terminal::TerminalManager;
use crate::ui::draw;

pub struct App {
    pub repo: GitRepo,
    pub worktrees: Vec<WorktreeInfo>,
    pub selected_index: usize,
    pub terminal_manager: TerminalManager,
    pub terminal_sessions: HashMap<String, TerminalManager>,
    pub active_terminal_path: String,
    pub focus: Focus,
    pub base_path: String,
    pub should_quit: bool,
    add_modal_state: AddWorktreeModal,
    progress_overlay: Option<String>,
    error_message: Option<String>,
    pending_action: Option<PendingAction>,
    confirm_dialog: Option<ConfirmDialog>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Terminal,
}

#[derive(Debug, Default)]
pub struct AddWorktreeModal {
    pub visible: bool,
    pub input: String,
    pub error: Option<String>,
    pub is_submitting: bool,
}

#[derive(Debug)]
enum PendingAction {
    AddWorktree { branch: String },
    RemoveWorktree { path: String },
    PruneWorktrees,
}

#[derive(Debug)]
enum ConfirmAction {
    RemoveWorktree { path: String },
    PruneWorktrees,
}

#[derive(Debug)]
pub struct ConfirmDialog {
    pub message: String,
    action: ConfirmAction,
}

impl App {
    pub fn new(repo_path: &str) -> Result<Self> {
        let repo = GitRepo::open(std::path::Path::new(repo_path))?;
        let worktrees = worktree::list_worktrees(&repo)?;
        let base_path = repo.repo_root()?.to_string_lossy().to_string();

        let selected_index = worktrees
            .iter()
            .position(|wt| wt.path == base_path)
            .unwrap_or(0);

        let terminal_manager = TerminalManager::new()?;

        let active_terminal_path = worktrees
            .get(selected_index)
            .map(|wt| wt.path.clone())
            .unwrap_or_else(|| base_path.clone());

        Ok(Self {
            repo,
            worktrees,
            selected_index,
            terminal_manager,
            terminal_sessions: HashMap::new(),
            active_terminal_path,
            focus: Focus::List,
            base_path,
            should_quit: false,
            add_modal_state: AddWorktreeModal::default(),
            progress_overlay: None,
            error_message: None,
            pending_action: None,
            confirm_dialog: None,
        })
    }

    pub async fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        loop {
            terminal.draw(|f| draw::<B>(f, self))?;

            if self.focus == Focus::Terminal && self.terminal_manager.is_disconnected() {
                self.focus = Focus::List;
            }

            self.process_pending_action();

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key.code, key.modifiers);
                }
            }

            if self.focus == Focus::Terminal {
                self.terminal_manager.update().await?;
            }

            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    // All handle_key, handle_list_key, handle_terminal_key, handle_add_modal_key,
    // handle_confirm_key, key_to_ansi methods remain identical to the existing
    // packages/tui/src/app.rs — copy them verbatim.
    //
    // The only changes are in process_pending_action and refresh_worktrees
    // which now use wt_core functions:

    fn refresh_worktrees(&mut self) {
        if let Ok(wts) = worktree::list_worktrees(&self.repo) {
            self.worktrees = wts;
            if self.selected_index >= self.worktrees.len() {
                self.selected_index = self.worktrees.len().saturating_sub(1);
            }
        }
    }

    fn process_pending_action(&mut self) {
        if let Some(action) = self.pending_action.take() {
            match action {
                PendingAction::AddWorktree { branch } => {
                    let opts = AddOptions {
                        branch: branch.clone(),
                        progress: false,
                        ..AddOptions::default()
                    };
                    match worktree::add_worktree(&self.repo, opts) {
                        Ok(_) => {
                            self.clear_error();
                            self.close_add_modal();
                            self.refresh_worktrees();
                            if let Some(idx) = self.worktrees.iter().position(|wt| {
                                wt.branch.as_deref() == Some(branch.as_str())
                            }) {
                                self.selected_index = idx;
                            }
                            self.update_terminal_for_selection();
                        }
                        Err(err) => {
                            self.set_error(format!("Failed to create worktree: {}", err));
                            self.add_modal_state.error = Some(err.to_string());
                            self.add_modal_state.is_submitting = false;
                        }
                    }
                }
                PendingAction::RemoveWorktree { path } => {
                    let opts = wt_core::types::RemoveOptions {
                        target: path.clone(),
                        force: false,
                        as_branch: false,
                        as_path: true,
                    };
                    match worktree::remove_worktree(&self.repo, opts) {
                        Ok(_) => {
                            self.clear_error();
                            self.refresh_worktrees();
                        }
                        Err(err) => {
                            self.set_error(format!("Failed to remove worktree: {}", err));
                        }
                    }
                }
                PendingAction::PruneWorktrees => {
                    let opts = wt_core::types::PruneOptions {
                        dry_run: false,
                        verbose: false,
                        expire: None,
                    };
                    let _ = worktree::prune_worktrees(&self.repo, opts);
                    self.refresh_worktrees();
                }
            }
            self.hide_progress_overlay();
        }
    }

    // Copy all remaining methods from packages/tui/src/app.rs verbatim:
    // handle_key, handle_list_key, handle_terminal_key, handle_add_modal_key,
    // handle_confirm_key, key_to_ansi, confirm_remove_selected,
    // confirm_prune_worktrees, update_terminal_for_selection,
    // switch_terminal_session, open_add_modal, close_add_modal,
    // submit_add_modal, add_modal, add_modal_visible, progress_overlay,
    // show_progress_overlay, hide_progress_overlay, error_message,
    // set_error, clear_error, confirm_message, execute_confirm_action
}
```

Note to implementer: Copy ALL methods from the existing `packages/tui/src/app.rs` lines 139-497. The only methods that change are `new()` (use `GitRepo::open`), `refresh_worktrees()` (use `worktree::list_worktrees`), and `process_pending_action()` (use `worktree::add_worktree` / `worktree::remove_worktree` / `worktree::prune_worktrees`). Everything else is identical.

- [ ] **Step 4: Copy ui.rs from existing TUI**

Copy `packages/tui/src/ui.rs` to `crates/wt-tui/src/ui.rs`. Change the import from `crate::app::Worktree` references — the `App` struct now has `worktrees: Vec<WorktreeInfo>` (from `wt_core::types`), but `WorktreeInfo` has the same fields (`path`, `branch`, `head`, `is_base`, `is_locked`, `is_prunable`) so `ui.rs` works unchanged.

Only update the import line:
```rust
use crate::app::{App, Focus};
```
(This is the same as before — no change needed.)

- [ ] **Step 5: Create crates/wt-tui/src/lib.rs**

```rust
// crates/wt-tui/src/lib.rs
pub mod app;
pub mod terminal;
pub mod ui;
```

- [ ] **Step 6: Create src/cmd/tui.rs**

```rust
// src/cmd/tui.rs
use std::io;

use anyhow::Result;
use clap::Args;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use wt_tui::app::App;

#[derive(Args, Debug)]
pub struct TuiArgs {
    /// Repository path
    #[arg(short, long, default_value = ".")]
    repo: String,
}

pub fn run(args: TuiArgs) -> Result<()> {
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let mut app = App::new(&args.repo)?;
        let res = app.run(&mut terminal).await;

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        if let Err(err) = res {
            eprintln!("Error: {:?}", err);
        }
        Ok(())
    })
}
```

- [ ] **Step 7: Update cmd/mod.rs and main.rs**

Add to `src/cmd/mod.rs`:
```rust
pub mod tui;
```

Add to `main.rs` Commands enum:
```rust
    /// Interactive TUI for worktrees
    Tui(cmd::tui::TuiArgs),
```

Add to match:
```rust
    Commands::Tui(args) => cmd::tui::run(args),
```

- [ ] **Step 8: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 9: Test the TUI**

Run: `cargo run -- tui`
Expected: TUI launches with worktree list, terminal pane, keyboard navigation all working

- [ ] **Step 10: Commit**

```bash
git add crates/wt-tui/ src/cmd/tui.rs src/cmd/mod.rs src/main.rs
git commit -m "feat: migrate TUI to wt-tui crate using wt-core"
```

---

### Task 9: Shell Completion

**Files:**
- Create: `src/cmd/completion.rs`
- Modify: `src/cmd/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create cmd/completion.rs**

```rust
// src/cmd/completion.rs
use std::fs;

use anyhow::Result;
use clap::{Args, Subcommand};
use wt_core::git::GitRepo;

use crate::output;

#[derive(Args, Debug)]
pub struct CompletionArgs {
    #[command(subcommand)]
    command: CompletionCommands,
}

#[derive(Subcommand, Debug)]
pub enum CompletionCommands {
    /// Print zsh completion script
    Zsh,
    /// Print bash completion script
    Bash,
    /// Print fish completion script
    Fish,
    /// Auto-detect shell and install completion
    Install {
        /// Shell name (zsh|fish|bash)
        #[arg(long)]
        shell: Option<String>,
    },
}

pub fn run(args: CompletionArgs) -> Result<()> {
    match args.command {
        CompletionCommands::Zsh => print!("{}", ZSH_COMPLETION),
        CompletionCommands::Bash => print!("{}", BASH_COMPLETION),
        CompletionCommands::Fish => print!("{}", FISH_COMPLETION),
        CompletionCommands::Install { shell } => {
            let shell = shell.unwrap_or_else(detect_shell).to_lowercase();
            match shell.as_str() {
                "zsh" => install_zsh()?,
                "fish" => install_fish()?,
                "bash" => install_bash()?,
                _ => {
                    eprintln!("Only zsh, fish, and bash completion are supported");
                    std::process::exit(1);
                }
            }
        }
    }
    Ok(())
}

/// Hidden subcommand: print branch names for dynamic completion
pub fn complete_branches() -> Result<()> {
    let repo = GitRepo::open(&std::env::current_dir()?)?;
    let branches = repo.list_branches()?;
    for b in branches {
        println!("{}", b);
    }
    Ok(())
}

/// Hidden subcommand: print worktree paths for dynamic completion
pub fn complete_worktrees() -> Result<()> {
    let repo = GitRepo::open(&std::env::current_dir()?)?;
    let paths = repo.list_worktree_paths()?;
    for p in paths {
        println!("{}", p);
    }
    Ok(())
}

/// Hidden subcommand: print action names for dynamic completion
pub fn complete_actions() {
    for action in &["add", "list", "remove", "switch", "tui", "prune", "uninstall"] {
        println!("{}", action);
    }
}

fn detect_shell() -> String {
    if std::env::var("ZSH_VERSION").is_ok() { return "zsh".into(); }
    if std::env::var("FISH_VERSION").is_ok() { return "fish".into(); }
    if std::env::var("BASH_VERSION").is_ok() { return "bash".into(); }
    std::env::var("SHELL")
        .ok()
        .and_then(|s| {
            std::path::Path::new(&s)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "zsh".into())
}

fn install_zsh() -> Result<()> {
    let home = dirs::home_dir().unwrap();
    let dir = home.join(".zsh").join("completions");
    let file = dir.join("_wt");
    let zshrc = home.join(".zshrc");

    fs::create_dir_all(&dir)?;
    fs::write(&file, ZSH_COMPLETION)?;

    let start_marker = "# wt completion start";
    let end_marker = "# wt completion end";
    let block = format!(
        "{}\nfpath=(~/.zsh/completions $fpath)\nautoload -Uz compinit\ncompinit\n{}\n",
        start_marker, end_marker
    );

    let existing = fs::read_to_string(&zshrc).unwrap_or_default();
    if !existing.contains(start_marker)
        && !existing.contains("fpath=(~/.zsh/completions $fpath)")
    {
        let next = if existing.is_empty() || existing.ends_with('\n') {
            format!("{}\n{}", existing, block)
        } else {
            format!("{}\n\n{}", existing, block)
        };
        fs::write(&zshrc, next)?;
    }

    output::log(&format!("Installed zsh completion:\n- {}", file.display()));
    println!("Reload your shell:\n- source ~/.zshrc");
    Ok(())
}

fn install_fish() -> Result<()> {
    let home = dirs::home_dir().unwrap();
    let dir = home.join(".config").join("fish").join("completions");
    let file = dir.join("wt.fish");

    fs::create_dir_all(&dir)?;
    fs::write(&file, FISH_COMPLETION)?;

    output::log(&format!("Installed fish completion:\n- {}", file.display()));
    println!("Reload fish or run:\n- source {}", file.display());
    Ok(())
}

fn install_bash() -> Result<()> {
    let home = dirs::home_dir().unwrap();
    let dir = home.join(".bash_completion.d");
    let file = dir.join("wt");
    let bashrc = home.join(".bashrc");

    fs::create_dir_all(&dir)?;
    fs::write(&file, BASH_COMPLETION)?;

    let start_marker = "# wt completion start";
    let end_marker = "# wt completion end";
    let file_str = file.to_string_lossy();
    let block = format!(
        "{}\nsource \"{}\"\n{}\n",
        start_marker, file_str, end_marker
    );

    let existing = fs::read_to_string(&bashrc).unwrap_or_default();
    if !existing.contains(start_marker) && !existing.contains(&format!("source \"{}\"", file_str))
    {
        let next = if existing.is_empty() || existing.ends_with('\n') {
            format!("{}\n{}", existing, block)
        } else {
            format!("{}\n\n{}", existing, block)
        };
        fs::write(&bashrc, next)?;
    }

    output::log(&format!("Installed bash completion:\n- {}", file.display()));
    println!("Reload your shell:\n- source ~/.bashrc");
    Ok(())
}

const FISH_COMPLETION: &str = r#"# fish completion for wt

function __wt_complete_actions
  wt __complete-actions
end

function __wt_complete_branches
  wt __complete-branches
end

function __wt_complete_worktrees
  wt __complete-worktrees
end

# top-level commands
complete -c wt -f -n "__fish_use_subcommand" -a "(__wt_complete_actions)"

# add
complete -c wt -f -n "__fish_seen_subcommand_from add" -a "(__wt_complete_branches)"
complete -c wt -f -n "__fish_seen_subcommand_from add" -s d -l dir -r -d "Target directory for the worktree"
complete -c wt -f -n "__fish_seen_subcommand_from add" -s n -l new-branch -d "Force creating a new branch even if remote branch exists"
complete -c wt -f -n "__fish_seen_subcommand_from add" -s b -l base -r -d "Base ref when creating a new branch"
complete -c wt -f -n "__fish_seen_subcommand_from add" -s r -l remote -r -d "Remote name"
complete -c wt -f -n "__fish_seen_subcommand_from add" -s f -l force -d "Allow if target directory already exists"
complete -c wt -f -n "__fish_seen_subcommand_from add" -l no-progress -d "Do not print step/progress messages"

# list
complete -c wt -f -n "__fish_seen_subcommand_from list" -l raw -d "Print raw git worktree list output"
complete -c wt -f -n "__fish_seen_subcommand_from list" -l json -d "Print JSON output"

# remove
complete -c wt -f -n "__fish_seen_subcommand_from remove" -a "(__wt_complete_worktrees)"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -a "(__wt_complete_branches)"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -s f -l force -d "Force removal"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -s b -l branch -d "Treat target as a branch name"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -s p -l path -r -d "Treat target as a worktree path"

# switch
complete -c wt -f -n "__fish_seen_subcommand_from switch" -a "(__wt_complete_worktrees)"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -a "(__wt_complete_branches)"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -s b -l branch -d "Treat target as a branch name"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -s p -l path -r -d "Treat target as a worktree path"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -l print -d "Print resolved worktree path only"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -l shell -r -d "Shell to use"

# tui
complete -c wt -f -n "__fish_seen_subcommand_from tui" -d "Interactive TUI for worktrees"

# prune
complete -c wt -f -n "__fish_seen_subcommand_from prune" -l dry-run -d "Do not remove anything; show what would be pruned"
complete -c wt -f -n "__fish_seen_subcommand_from prune" -l verbose -d "Report all removals"
complete -c wt -f -n "__fish_seen_subcommand_from prune" -l expire -r -d "Expire worktrees older than <time>"

# completion
complete -c wt -f -n "__fish_seen_subcommand_from completion" -a "zsh fish bash install"
complete -c wt -f -n "__fish_seen_subcommand_from completion install" -l shell -r -a "zsh fish bash" -d "Shell name"

# uninstall
complete -c wt -f -n "__fish_seen_subcommand_from uninstall" -l shell -r -a "zsh fish bash all" -d "Shell name"
complete -c wt -f -n "__fish_seen_subcommand_from uninstall" -l yes -d "Do not prompt"
"#;

const BASH_COMPLETION: &str = r#"# bash completion for wt

_wt()
{
  local cur prev words cword
  _init_completion -n : || return

  if [[ $cword -eq 1 ]]; then
    COMPREPLY=( $(compgen -W "$(wt __complete-actions)" -- "$cur") )
    return
  fi

  local cmd=${words[1]}
  case "$cmd" in
    add)
      COMPREPLY=( $(compgen -W "$(wt __complete-branches)" -- "$cur") )
      return
      ;;
    remove|switch)
      COMPREPLY=( $(compgen -W "$(wt __complete-worktrees) $(wt __complete-branches)" -- "$cur") )
      return
      ;;
    completion)
      if [[ $cword -eq 2 ]]; then
        COMPREPLY=( $(compgen -W "zsh fish bash install" -- "$cur") )
        return
      fi
      return
      ;;
  esac
}

if declare -F complete >/dev/null 2>&1; then
  complete -F _wt wt
fi
"#;

const ZSH_COMPLETION: &str = r#"#compdef wt
_wt() {
  local -a commands
  commands=(
    'add:Add a new worktree from a branch'
    'list:List all worktrees'
    'remove:Delete a worktree'
    'switch:Switch to a worktree and open a shell in its directory'
    'tui:Interactive TUI for worktrees'
    'completion:Shell completion utilities'
    'uninstall:Remove wt shell completions and print package uninstall instructions'
  )

  _arguments -C \
    '1:command:->command' \
    '*::arg:->args'

  case $state in
    (command)
      _describe 'command' commands
      return
    ;;
  esac

  case $words[1] in
    (add)
      _arguments \
        '1:branch:($(wt __complete-branches))' \
        '(-d --dir)'{-d,--dir}'[Target directory for the worktree]:path:_files -/' \
        '(-n --new-branch)'{-n,--new-branch}'[Force creating a new branch even if remote branch exists]' \
        '(-b --base)'{-b,--base}'[Base ref when creating a new branch]:ref:' \
        '(-r --remote)'{-r,--remote}'[Remote name]:remote:' \
        '(-f --force)'{-f,--force}'[Allow if target directory already exists]' \
        '(--no-progress)--no-progress[Do not print step/progress messages]'
      return
    ;;
    (list)
      _arguments \
        '(--raw)--raw[Print raw git worktree list output]' \
        '(--json)--json[Print JSON output]'
      return
    ;;
    (remove)
      _arguments \
        '1:target:($(wt __complete-worktrees))' \
        '(-f --force)'{-f,--force}'[Force removal]' \
        '(-b --branch)'{-b,--branch}'[Treat target as a branch name]' \
        '(-p --path)'{-p,--path}'[Treat target as a worktree path]:path:_files -/'
      return
    ;;
    (switch)
      _arguments \
        '1:target:($(wt __complete-worktrees))' \
        '(-b --branch)'{-b,--branch}'[Treat target as a branch name]' \
        '(-p --path)'{-p,--path}'[Treat target as a worktree path]:path:_files -/' \
        '(--print)--print[Print resolved worktree path only]' \
        '(--shell)--shell[Shell to use]:shell:'
      return
    ;;
    (tui)
      _arguments
      return
    ;;
    (prune)
      _arguments \
        '(--dry-run)--dry-run[Do not remove anything; show what would be pruned]' \
        '(--verbose)--verbose[Report all removals]' \
        '(--expire)--expire[Expire worktrees older than <time>]:time:'
      return
    ;;
    (completion)
      _arguments \
        '1:subcommand:(zsh fish bash install)'
      return
    ;;
    (uninstall)
      _arguments \
        '(--shell)--shell[Shell name]:shell:(zsh fish bash all)' \
        '(--yes)--yes[Do not prompt]'
      return
    ;;
  esac
}

_wt
"#;
```

- [ ] **Step 2: Update cmd/mod.rs**

```rust
// src/cmd/mod.rs
pub mod add;
pub mod completion;
pub mod list;
pub mod prune;
pub mod remove;
pub mod switch;
pub mod tui;
pub mod uninstall;
```

- [ ] **Step 3: Update main.rs — add completion + hidden subcommands**

```rust
// src/main.rs
mod cmd;
mod output;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "wt", about = "Git worktree manager", version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new worktree from a branch
    Add(cmd::add::AddArgs),
    /// List all worktrees
    List(cmd::list::ListArgs),
    /// Delete a worktree
    #[command(alias = "rm")]
    Remove(cmd::remove::RemoveArgs),
    /// Switch to a worktree and open a shell in its directory
    #[command(alias = "sw")]
    Switch(cmd::switch::SwitchArgs),
    /// Interactive TUI for worktrees
    Tui(cmd::tui::TuiArgs),
    /// Prune stale worktree information
    Prune(cmd::prune::PruneArgs),
    /// Shell completion utilities
    Completion(cmd::completion::CompletionArgs),
    /// Remove wt shell completions and print uninstall instructions
    Uninstall(cmd::uninstall::UninstallArgs),

    // Hidden completion helpers
    #[command(name = "__complete-branches", hide = true)]
    CompleteBranches,
    #[command(name = "__complete-worktrees", hide = true)]
    CompleteWorktrees,
    #[command(name = "__complete-actions", hide = true)]
    CompleteActions,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Add(args) => cmd::add::run(args),
        Commands::List(args) => cmd::list::run(args),
        Commands::Remove(args) => cmd::remove::run(args),
        Commands::Switch(args) => cmd::switch::run(args),
        Commands::Tui(args) => cmd::tui::run(args),
        Commands::Prune(args) => cmd::prune::run(args),
        Commands::Completion(args) => cmd::completion::run(args),
        Commands::Uninstall(args) => cmd::uninstall::run(args),
        Commands::CompleteBranches => cmd::completion::complete_branches(),
        Commands::CompleteWorktrees => cmd::completion::complete_worktrees(),
        Commands::CompleteActions => { cmd::completion::complete_actions(); Ok(()) }
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors

- [ ] **Step 5: Test completions**

Run: `cargo run -- __complete-actions`
Expected: prints add, list, remove, switch, tui, prune, uninstall

Run: `cargo run -- __complete-branches`
Expected: prints branch names

Run: `cargo run -- completion fish`
Expected: prints fish completion script

- [ ] **Step 6: Commit**

```bash
git add src/cmd/completion.rs src/cmd/mod.rs src/main.rs
git commit -m "feat: add shell completion (zsh, bash, fish) with dynamic branch/worktree lookup"
```

---

### Task 10: Cleanup — Remove Old Packages

**Files:**
- Delete: `packages/core/`
- Delete: `packages/mcp/`
- Delete: `packages/tui/`
- Delete: `pnpm-workspace.yaml`
- Delete: `tsconfig.json`
- Modify: `package.json` (remove or repurpose)
- Modify: `.gitignore`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Remove old packages**

```bash
rm -rf packages/core packages/mcp packages/tui
rm -f pnpm-workspace.yaml tsconfig.json
```

- [ ] **Step 2: Update .gitignore**

Replace the existing `.gitignore` content with:

```
target/
node_modules/
dist/
*.tgz
```

- [ ] **Step 3: Verify the Rust build still works**

Run: `cargo build`
Expected: builds successfully

Run: `cargo run -- list`
Expected: works correctly

Run: `cargo run -- tui`
Expected: TUI launches correctly

- [ ] **Step 4: Update CLAUDE.md**

Update to reflect the new Rust-only structure:

```markdown
# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`wt` is a git worktree manager — a single Rust binary with CLI commands and an interactive TUI.

## Structure

- **crates/wt-core** — lib crate: git operations (git2), TOML config, hooks, env copying, worktree orchestration
- **crates/wt-tui** — lib crate: Ratatui TUI (app state, rendering, PTY sessions)
- **src/** — bin crate: clap CLI routing, per-command modules in `src/cmd/`
- **packages/npm/** — lightweight npm wrapper that downloads prebuilt binaries

## Build Commands

\`\`\`bash
cargo build                    # debug build
cargo build --release          # release build
cargo run -- <subcommand>      # run in dev
cargo check                    # type check all crates
\`\`\`

No test suite yet.

## Architecture Notes

- **Single binary**: CLI commands and TUI share `wt-core` — no duplicated git logic.
- **CLI is synchronous**: no tokio runtime. `wt tui` initializes tokio on demand for PTY sessions.
- **Hook system**: TOML-based lifecycle hooks (pre_create, post_create) in `.wt.toml`. Variables: `${base}`, `${worktree}`, env vars `WT_BASE`, `WT_WORKTREE`, `WT_BRANCH`.
- **Config resolution**: `.wt.toml` → `wt.toml` → `.config/wt/config.toml` → `~/.config/wt/config.toml` → `$XDG_CONFIG_HOME/wt/config.toml`.
- **Shell completion**: hand-written scripts (zsh/bash/fish) with hidden subcommands for dynamic branch/worktree lookup.
\`\`\`

```

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: remove TypeScript packages, update docs for Rust-only codebase"
```

---

### Task 11: npm Wrapper + GitHub Actions Release

**Files:**
- Create: `packages/npm/package.json`
- Create: `packages/npm/postinstall.js`
- Create: `.github/workflows/release.yml`
- Create: `install.sh`

- [ ] **Step 1: Create packages/npm/package.json**

```json
{
  "name": "@listennn08/wt",
  "version": "1.0.0",
  "description": "Git worktree manager (npm wrapper — downloads prebuilt Rust binary)",
  "bin": {
    "wt": "./bin/wt"
  },
  "scripts": {
    "postinstall": "node postinstall.js"
  },
  "os": ["darwin", "linux"],
  "cpu": ["x64", "arm64"]
}
```

- [ ] **Step 2: Create packages/npm/postinstall.js**

```javascript
#!/usr/bin/env node
const { execSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");
const https = require("https");

const REPO = "listennn08/wt";
const BIN_DIR = path.join(__dirname, "bin");

function getPlatformTarget() {
  const platform = os.platform();
  const arch = os.arch();
  const map = {
    "darwin-x64": "x86_64-apple-darwin",
    "darwin-arm64": "aarch64-apple-darwin",
    "linux-x64": "x86_64-unknown-linux-gnu",
    "linux-arm64": "aarch64-unknown-linux-gnu",
  };
  return map[`${platform}-${arch}`];
}

async function download(url, dest) {
  return new Promise((resolve, reject) => {
    const follow = (url) => {
      https.get(url, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          follow(res.headers.location);
          return;
        }
        if (res.statusCode !== 200) {
          reject(new Error(`Download failed: HTTP ${res.statusCode}`));
          return;
        }
        const file = fs.createWriteStream(dest);
        res.pipe(file);
        file.on("finish", () => { file.close(); resolve(); });
      }).on("error", reject);
    };
    follow(url);
  });
}

async function main() {
  const target = getPlatformTarget();
  if (!target) {
    console.error(`Unsupported platform: ${os.platform()}-${os.arch()}`);
    process.exit(1);
  }

  const pkg = require("./package.json");
  const version = pkg.version;
  const assetName = `wt-${target}.tar.gz`;
  const url = `https://github.com/${REPO}/releases/download/v${version}/${assetName}`;

  fs.mkdirSync(BIN_DIR, { recursive: true });
  const tarball = path.join(BIN_DIR, assetName);

  console.log(`Downloading wt v${version} for ${target}...`);
  await download(url, tarball);
  execSync(`tar -xzf "${tarball}" -C "${BIN_DIR}"`, { stdio: "inherit" });
  fs.unlinkSync(tarball);
  fs.chmodSync(path.join(BIN_DIR, "wt"), 0o755);
  console.log("wt installed successfully");
}

main().catch((err) => {
  console.error("Failed to install wt:", err.message);
  process.exit(1);
});
```

- [ ] **Step 3: Create .github/workflows/release.yml**

```yaml
name: Release

on:
  push:
    tags:
      - "v*"

permissions:
  contents: write

jobs:
  build:
    strategy:
      matrix:
        include:
          - target: x86_64-apple-darwin
            os: macos-latest
          - target: aarch64-apple-darwin
            os: macos-latest
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-latest

    runs-on: ${{ matrix.os }}

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install cross-compilation tools (Linux ARM)
        if: matrix.target == 'aarch64-unknown-linux-gnu'
        run: |
          sudo apt-get update
          sudo apt-get install -y gcc-aarch64-linux-gnu

      - name: Build release binary
        run: cargo build --release --target ${{ matrix.target }}
        env:
          CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER: aarch64-linux-gnu-gcc

      - name: Package
        run: |
          cd target/${{ matrix.target }}/release
          tar -czf ../../../wt-${{ matrix.target }}.tar.gz wt

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: wt-${{ matrix.target }}
          path: wt-${{ matrix.target }}.tar.gz

  release:
    needs: build
    runs-on: ubuntu-latest
    steps:
      - uses: actions/download-artifact@v4
        with:
          merge-multiple: true

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          files: wt-*.tar.gz
          generate_release_notes: true
```

- [ ] **Step 4: Create install.sh**

```bash
#!/bin/sh
set -e

REPO="listennn08/wt"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

detect_target() {
  OS=$(uname -s | tr '[:upper:]' '[:lower:]')
  ARCH=$(uname -m)

  case "${OS}-${ARCH}" in
    darwin-x86_64)  echo "x86_64-apple-darwin" ;;
    darwin-arm64)   echo "aarch64-apple-darwin" ;;
    linux-x86_64)   echo "x86_64-unknown-linux-gnu" ;;
    linux-aarch64)  echo "aarch64-unknown-linux-gnu" ;;
    *) echo "Unsupported platform: ${OS}-${ARCH}" >&2; exit 1 ;;
  esac
}

TARGET=$(detect_target)
VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed 's/.*"v\(.*\)".*/\1/')
URL="https://github.com/${REPO}/releases/download/v${VERSION}/wt-${TARGET}.tar.gz"

echo "Installing wt v${VERSION} for ${TARGET}..."
TMP=$(mktemp -d)
curl -fsSL "${URL}" | tar -xz -C "${TMP}"
install -m 755 "${TMP}/wt" "${INSTALL_DIR}/wt"
rm -rf "${TMP}"
echo "wt installed to ${INSTALL_DIR}/wt"
```

- [ ] **Step 5: Commit**

```bash
chmod +x install.sh
git add packages/npm/ .github/ install.sh
git commit -m "feat: add npm wrapper, GitHub Actions release workflow, and install script"
```
