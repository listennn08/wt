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

// ponytail: counts chars, not display width. Good enough until someone puts a
// wide CJK char or emoji in a branch name; reach for unicode-width if that happens.
fn pad(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - len))
    }
}

/// Truncate from the left, keeping the tail (paths are most specific at the end).
fn truncate_start(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len <= width || width == 0 {
        return s.to_string();
    }
    let tail: String = s.chars().skip(len - (width - 1)).collect();
    format!("…{}", tail)
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

    let path_width = rows
        .iter()
        .map(|r| r.0.chars().count())
        .max()
        .unwrap_or(4)
        .clamp(4, 60);
    let branch_width = rows
        .iter()
        .map(|r| r.1.chars().count())
        .max()
        .unwrap_or(6)
        .clamp(6, 30);

    // Header
    let header = format!(
        "{}  {}  {}  FLAGS",
        pad("BRANCH", branch_width),
        pad("PATH", path_width),
        pad("HEAD", 8),
    );
    println!("{}", header.bold().dimmed());

    for (path, branch, head, flags, is_base) in &rows {
        let p = truncate_start(path, path_width);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_and_truncates_by_chars_not_bytes() {
        assert_eq!(pad("ab", 4), "ab  ");
        assert_eq!(pad("中文", 4), "中文  ");
        assert_eq!(pad("toolong", 3), "toolong");

        // Would panic on a byte-index slice: each char here is 3 bytes.
        assert_eq!(truncate_start("中文测试", 3), "…测试");
        assert_eq!(truncate_start("/a/b/c", 4), "…b/c");
        assert_eq!(truncate_start("short", 10), "short");
        assert_eq!(truncate_start("x", 0), "x");
    }
}
