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
