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
        .unwrap_or_else(detect_shell)
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
