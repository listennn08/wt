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
            pad("", branch_width)
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
