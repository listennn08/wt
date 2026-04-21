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
