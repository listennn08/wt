import { SimpleGit } from "simple-git";
import { AbstractCommand } from "./base";
import { Command } from "commander";
import fs from 'fs';
import os from 'os';
import path from 'path';

export class CompletionCommand extends AbstractCommand {
  constructor(protected git: SimpleGit) { super(git) }

  public load(program: Command): void {
    const completion = program
      .command('completion')
      .description('Shell completion utilities');

    completion
      .command('zsh')
      .description('Print zsh completion script')
      .action(async () => {
        process.stdout.write(this.zshCompletionScript());
      });

    completion
      .command('fish')
      .description('Print fish completion script')
      .action(async () => {
        process.stdout.write(this.fishCompletionScript());
      });

    completion
      .command('bash')
      .description('Print bash completion script')
      .action(async () => {
        process.stdout.write(this.bashCompletionScript());
      });

    completion
      .command('install')
      .description('Auto-detect shell and install completion')
      .option('--shell <shell>', 'Shell name (zsh|fish|bash)')
      .action(async (options: { shell?: string }) => {
        const shell = (options.shell ?? this.detectShell()).toLowerCase();
        if (shell === 'zsh') {
          this.installZshCompletion();
          return;
        }
        if (shell === 'fish') {
          this.installFishCompletion();
          return;
        }

        if (shell === 'bash') {
          this.installBashCompletion();
          return;
        }

        console.error('Only zsh, fish, and bash completion are supported');
        process.exit(1);
      });

    program.command('__complete-actions', { hidden: true })
      .description('Print actions for shell completion')
      .action(async () => {
        process.stdout.write(['add', 'list', 'remove', 'switch', 'tui', 'prune', 'uninstall'].join('\n'));
        process.stdout.write('\n');
      });
    program
      .command('__complete-branches', { hidden: true })
      .description('Print branches for shell completion')
      .action(async () => {
        const branches = await this.branchesForCompletion();
        process.stdout.write(branches.join('\n'));
        if (branches.length > 0) process.stdout.write('\n');
      });

    program
      .command('__complete-worktrees', { hidden: true })
      .description('Print worktree paths for shell completion')
      .action(async () => {
        const worktrees = await this.worktreesForCompletion();
        process.stdout.write(worktrees.join('\n'));
        if (worktrees.length > 0) process.stdout.write('\n');
      });
  }


  private async branchesForCompletion(): Promise<string[]> {
    const out = await this.git.raw([
      'for-each-ref',
      '--format=%(refname:short)',
      'refs/heads',
      'refs/remotes',
    ]);

    const branches = out
      .split(/\r?\n/)
      .map((s) => s.trim())
      .filter(Boolean)
      .filter((b) => !b.endsWith('/HEAD'));

    return Array.from(new Set(branches)).sort();
  }

  private async worktreesForCompletion(): Promise<string[]> {
    const baseTop = (await this.git.revparse(['--show-toplevel'])).trim();
    const out = await this.git.raw(['worktree', 'list', '--porcelain']);

    const paths: string[] = [];
    for (const line of out.split(/\r?\n/)) {
      const trimmed = line.trim();
      if (!trimmed.startsWith('worktree ')) continue;
      const p = trimmed.slice('worktree '.length).trim();
      if (!p) continue;
      const abs = path.resolve(p);
      if (abs === path.resolve(baseTop)) continue;
      paths.push(abs);
    }

    return Array.from(new Set(paths)).sort();
  }

  private detectShell(): string {
    if (process.env.ZSH_VERSION) return 'zsh';
    if (process.env.FISH_VERSION) return 'fish';
    if (process.env.BASH_VERSION) return 'bash';

    const env = process.env.SHELL;
    if (env) return path.basename(env);
    return 'zsh';
  }

  private installZshCompletion(): void {
    const home = os.homedir();
    const completionDir = path.join(home, '.zsh', 'completions');
    const completionFile = path.join(completionDir, '_wt');
    const zshrcPath = path.join(home, '.zshrc');

    fs.mkdirSync(completionDir, { recursive: true });
    fs.writeFileSync(completionFile, this.zshCompletionScript(), { encoding: 'utf8' });

    const startMarker = '# wt completion start';
    const endMarker = '# wt completion end';
    const block = [
      startMarker,
      'fpath=(~/.zsh/completions $fpath)',
      'autoload -Uz compinit',
      'compinit',
      endMarker,
      '',
    ].join('\n');

    let existing = '';
    if (fs.existsSync(zshrcPath)) {
      existing = fs.readFileSync(zshrcPath, 'utf8');
    }

    const hasMarker = existing.includes(startMarker) || existing.includes(endMarker);
    const hasFpath = existing.includes('fpath=(~/.zsh/completions $fpath)');

    if (!hasMarker && !hasFpath) {
      const next = existing.length > 0 && !existing.endsWith('\n') ? existing + '\n' : existing;
      fs.writeFileSync(zshrcPath, next + '\n' + block, { encoding: 'utf8' });
    }

    process.stdout.write(`Installed zsh completion:\n- ${completionFile}\n`);
    process.stdout.write(`Reload your shell:\n- source ~/.zshrc\n`);
  }

  private installFishCompletion(): void {
    const home = os.homedir();
    const completionDir = path.join(home, '.config', 'fish', 'completions');
    const completionFile = path.join(completionDir, 'wt.fish');

    fs.mkdirSync(completionDir, { recursive: true });
    fs.writeFileSync(completionFile, this.fishCompletionScript(), { encoding: 'utf8' });

    process.stdout.write(`Installed fish completion:\n- ${completionFile}\n`);
    process.stdout.write(`Reload fish (new shell) or run:\n- source ${completionFile}\n`);
  }

  private installBashCompletion(): void {
    const home = os.homedir();
    const completionDir = path.join(home, '.bash_completion.d');
    const completionFile = path.join(completionDir, 'wt');
    const bashrcPath = path.join(home, '.bashrc');

    fs.mkdirSync(completionDir, { recursive: true });
    fs.writeFileSync(completionFile, this.bashCompletionScript(), { encoding: 'utf8' });

    const startMarker = '# wt completion start';
    const endMarker = '# wt completion end';
    const block = [
      startMarker,
      `source "${completionFile}"`,
      endMarker,
      '',
    ].join('\n');

    let existing = '';
    if (fs.existsSync(bashrcPath)) {
      existing = fs.readFileSync(bashrcPath, 'utf8');
    }

    const hasMarker = existing.includes(startMarker) || existing.includes(endMarker);
    const hasSource = existing.includes(`source "${completionFile}"`) || existing.includes(`. "${completionFile}"`);

    if (!hasMarker && !hasSource) {
      const next = existing.length > 0 && !existing.endsWith('\n') ? existing + '\n' : existing;
      fs.writeFileSync(bashrcPath, next + '\n' + block, { encoding: 'utf8' });
    }

    process.stdout.write(`Installed bash completion:\n- ${completionFile}\n`);
    process.stdout.write(`Reload your shell:\n- source ~/.bashrc\n`);
  }

  private fishCompletionScript(): string {
    return `# fish completion for wt

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
complete -c wt -f -n "__fish_seen_subcommand_from add" -l no-install -d "Do not run package install in the new worktree"
complete -c wt -f -n "__fish_seen_subcommand_from add" -l no-progress -d "Do not print step/progress messages"

# list
complete -c wt -f -n "__fish_seen_subcommand_from list" -l raw -d "Print raw git worktree list output"
complete -c wt -f -n "__fish_seen_subcommand_from list" -l json -d "Print JSON output"

# remove
complete -c wt -f -n "__fish_seen_subcommand_from remove" -a "(__wt_complete_worktrees)"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -a "(__wt_complete_branches)"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -s f -l force -d "Force removal"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -s b -l branch -r -a "(__wt_complete_branches)" -d "Treat target as a branch name"
complete -c wt -f -n "__fish_seen_subcommand_from remove" -s p -l path -r -d "Treat target as a worktree path"

# switch
complete -c wt -f -n "__fish_seen_subcommand_from switch" -a "(__wt_complete_worktrees)"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -a "(__wt_complete_branches)"
complete -c wt -f -n "__fish_seen_subcommand_from switch" -s b -l branch -r -a "(__wt_complete_branches)" -d "Treat target as a branch name"
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
complete -c wt -f -n "__fish_seen_subcommand_from uninstall" -l shell -r -a "zsh fish all" -d "Shell name"
complete -c wt -f -n "__fish_seen_subcommand_from uninstall" -l yes -d "Do not prompt"
`;
  }

  private bashCompletionScript(): string {
    return `# bash completion for wt

_wt()
{
  local cur prev words cword
  _init_completion -n : || return

  if [[ $cword -eq 1 ]]; then
    COMPREPLY=( $(compgen -W "$(wt __complete-actions)" -- "$cur") )
    return
  fi

  local cmd=\${words[1]}
  case "$cmd" in
    add)
      COMPREPLY=( $(compgen -W "$(wt __complete-branches)" -- "$cur") )
      return
      ;;
    remove|switch)
      COMPREPLY=( $(compgen -W "$(wt __complete-worktrees)\n$(wt __complete-branches)" -- "$cur") )
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
`;
  }

  private zshCompletionScript(): string {
    return `#compdef wt
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
              '(--no-install)--no-install[Do not run package install in the new worktree]' \
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
              '(-b --branch)'{-b,--branch}'[Treat target as a branch name]:branch:($(wt __complete-branches))' \
              '(-p --path)'{-p,--path}'[Treat target as a worktree path]:path:_files -/'
            return
          ;;
          (switch)
            _arguments \
              '1:target:($(wt __complete-worktrees))' \
              '(-b --branch)'{-b,--branch}'[Treat target as a branch name]:branch:($(wt __complete-branches))' \
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
              '(--shell)--shell[Shell name]:shell:(zsh fish all)' \
              '(--yes)--yes[Do not prompt]'
            return
          ;;
        esac
      }

    _wt
    `;
  }
}