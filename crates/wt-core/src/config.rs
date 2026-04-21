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
