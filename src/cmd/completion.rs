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

pub fn complete_branches() -> Result<()> {
    let repo = GitRepo::open(&std::env::current_dir()?)?;
    let branches = repo.list_branches()?;
    for b in branches {
        println!("{}", b);
    }
    Ok(())
}

pub fn complete_worktrees() -> Result<()> {
    let repo = GitRepo::open(&std::env::current_dir()?)?;
    let paths = repo.list_worktree_paths()?;
    for p in paths {
        println!("{}", p);
    }
    Ok(())
}

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

complete -c wt -f -n "__fish_use_subcommand" -a "(__wt_complete_actions)"

complete -c wt -f -n "__fish_seen_subcommand_from add" -a "(__wt_complete_branches)"
complete -c wt -f -n "__fish_seen_subcommand_from add" -s d -l dir -r -d "Target directory for the worktree"
complete -c wt -f -n "__fish_seen_subcommand_from add" -s n -l new-branch -d "Force creating a new branch even if remote branch exists"
complete -c wt -f -n "__fish_seen_subcommand_from add" -s b -l base -r -d "Base ref when creating a new branch"
complete -c wt -f -n "__fish_seen_subcommand_from add" -s r -l remote -r -d "Remote name"
complete -c wt -f -n "__fish_seen_subcommand_from add" -s f -l force -d "Allow if target directory already exists"
complete -c wt -f -n "__fish_seen_subcommand_from add" -l no-progress -d "Do not print step/progress messages"

complete -c wt -f -n "__fish_seen_subcommand_from list" -l raw -d "Print raw git worktree list output"
complete -c wt -f -n "__fish_seen_subcommand_from list" -l json -d "Print JSON output"

complete -c wt -f -n "__fish_seen_subcommand_from remove" -a "(__wt_complete_worktrees)"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -a "(__wt_complete_branches)"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -s f -l force -d "Force removal"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -s b -l branch -d "Treat target as a branch name"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -s p -l path -r -d "Treat target as a worktree path"

complete -c wt -f -n "__fish_seen_subcommand_from switch" -a "(__wt_complete_worktrees)"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -a "(__wt_complete_branches)"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -s b -l branch -d "Treat target as a branch name"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -s p -l path -r -d "Treat target as a worktree path"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -l print -d "Print resolved worktree path only"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -l shell -r -d "Shell to use"

complete -c wt -f -n "__fish_seen_subcommand_from tui" -d "Interactive TUI for worktrees"

complete -c wt -f -n "__fish_seen_subcommand_from prune" -l dry-run -d "Do not remove anything; show what would be pruned"
complete -c wt -f -n "__fish_seen_subcommand_from prune" -l verbose -d "Report all removals"
complete -c wt -f -n "__fish_seen_subcommand_from prune" -l expire -r -d "Expire worktrees older than <time>"

complete -c wt -f -n "__fish_seen_subcommand_from completion" -a "zsh fish bash install"
complete -c wt -f -n "__fish_seen_subcommand_from completion install" -l shell -r -a "zsh fish bash" -d "Shell name"

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
