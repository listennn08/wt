import { Command } from 'commander';
import fs from 'fs';
import os from 'os';
import path from 'path';
import { execFileSync } from 'child_process';
import { SimpleGit } from 'simple-git';
import { AbstractCommand } from './base';

export class UninstallCommand extends AbstractCommand {
  constructor(protected git: SimpleGit) { super(git); }

  public load(program: Command): void {
    program
      .command('uninstall')
      .description('Remove wt shell completions and print package uninstall instructions')
      .option('--shell <shell>', 'Shell name (zsh|fish|all)')
      .option('--yes', 'Do not prompt (currently no prompts)')
      .action(async (options: { shell?: string }) => {
        await this.uninstall(options);
      });
  }

  private detectShell(): string {
    if (process.env.ZSH_VERSION) return 'zsh';
    if (process.env.FISH_VERSION) return 'fish';
    if (process.env.BASH_VERSION) return 'bash';

    const env = process.env.SHELL;
    if (env) return path.basename(env);
    return 'zsh';
  }

  private resolveWtBinaryPath(): string | null {
    try {
      const out = execFileSync('which', ['wt'], { encoding: 'utf8' });
      const p = out.trim();
      return p.length > 0 ? p : null;
    } catch {
      // ignore
    }

    try {
      const out = execFileSync('whereis', ['-b', 'wt'], { encoding: 'utf8' });
      const tokens = out
        .trim()
        .split(/\s+/)
        .map((s) => s.trim())
        .filter(Boolean);

      const paths = tokens.filter((t) => t.includes('/'));
      if (paths.length === 0) return null;
      return paths[0] ?? null;
    } catch {
      return null;
    }
  }

  private uninstallCommandHint(): string {
    const ua = process.env.npm_config_user_agent ?? '';
    if (ua.includes('pnpm')) return 'pnpm remove -g wt';
    if (ua.includes('yarn')) return 'yarn global remove wt';
    if (ua.includes('bun')) return 'bun remove -g wt';
    return 'npm uninstall -g wt';
  }

  private uninstallCommandHintsForBinary(binaryPath: string | null): string[] {
    const hints: string[] = [];

    if (binaryPath) {
      if (binaryPath.includes('/opt/homebrew/') || binaryPath.includes('/usr/local/')) {
        hints.push('brew uninstall wt');
      }
      if (binaryPath.includes('/Library/pnpm/') || binaryPath.includes('/pnpm/')) {
        hints.push('pnpm remove -g wt');
      }
    }

    hints.push(this.uninstallCommandHint());

    return Array.from(new Set(hints));
  }

  private uninstallZshCompletion(): void {
    const home = os.homedir();
    const completionFile = path.join(home, '.zsh', 'completions', '_wt');
    const zshrcPath = path.join(home, '.zshrc');

    if (fs.existsSync(completionFile)) {
      try {
        fs.unlinkSync(completionFile);
        this.log(`removed ${completionFile}`);
      } catch {
        this.log(`failed to remove ${completionFile}`);
      }
    }

    const startMarker = '# wt completion start';
    const endMarker = '# wt completion end';

    if (!fs.existsSync(zshrcPath)) return;

    try {
      const existing = fs.readFileSync(zshrcPath, 'utf8');
      const start = existing.indexOf(startMarker);
      const end = existing.indexOf(endMarker);

      if (start === -1 || end === -1 || end < start) return;

      const afterEnd = existing.indexOf('\n', end);
      const cutEnd = afterEnd === -1 ? existing.length : afterEnd + 1;

      const next = (existing.slice(0, start) + existing.slice(cutEnd)).replace(/\n{3,}/g, '\n\n');
      fs.writeFileSync(zshrcPath, next, { encoding: 'utf8' });
      this.log(`updated ${zshrcPath}`);
    } catch {
      this.log(`failed to update ${zshrcPath}`);
    }
  }

  private uninstallFishCompletion(): void {
    const home = os.homedir();
    const completionFile = path.join(home, '.config', 'fish', 'completions', 'wt.fish');

    if (!fs.existsSync(completionFile)) return;

    try {
      fs.unlinkSync(completionFile);
      this.log(`removed ${completionFile}`);
    } catch {
      this.log(`failed to remove ${completionFile}`);
    }
  }

  private async uninstall(options: { shell?: string }): Promise<void> {
    const shell = (options.shell ?? this.detectShell()).toLowerCase();

    if (shell === 'zsh') {
      this.uninstallZshCompletion();
    } else if (shell === 'fish') {
      this.uninstallFishCompletion();
    } else if (shell === 'all') {
      this.uninstallZshCompletion();
      this.uninstallFishCompletion();
    } else {
      console.error('Only zsh, fish, and all are supported');
      process.exit(1);
    }

    const binary = this.resolveWtBinaryPath();
    if (binary) {
      process.stdout.write(`wt binary:\n- ${binary}\n`);
    }

    process.stdout.write('Uninstall package (if installed globally):\n');
    for (const hint of this.uninstallCommandHintsForBinary(binary)) {
      process.stdout.write(`- ${hint}\n`);
    }
  }
}
