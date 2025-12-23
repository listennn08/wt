import { Command } from "commander";
import fs from 'fs';
import path from 'path';
import { spawn } from 'child_process';
import { SimpleGit } from "simple-git";
import { AbstractCommand } from "./base";

export class SwitchCommand extends AbstractCommand {
  constructor(protected git: SimpleGit) { super(git); }

  public load(program: Command): void {
    program
      .command('switch')
      .alias('sw')
      .description('Switch to a worktree and open a shell in its directory')
      .argument('<target>', 'Worktree path or branch name')
      .option('-b, --branch <branch>', 'Treat <target> as a branch name')
      .option('-p, --path <path>', 'Treat <target> as a worktree path')
      .option('--print', 'Print resolved worktree path only')
      .option('--shell <shell>', 'Shell to use (default: $SHELL)')
      .action(async (target: string, options: { branch?: string; path?: string; print?: boolean; shell?: string }) => {
        await this.switchWorktreeTarget(target, options);
      });
  }

  private sanitizeBranchName(branch: string): string {
    return branch.trim().replace(/\s+/g, '-').replace(/[\\/]+/g, '-');
  }

  private async repoName(): Promise<string> {
    const top = (await this.git.revparse(['--show-toplevel'])).trim();
    return path.basename(top);
  }

  private async defaultWorktreeDirFromBranch(branch: string): Promise<string> {
    const top = (await this.git.revparse(['--show-toplevel'])).trim();
    const parent = path.dirname(top);
    const name = await this.repoName();
    return path.join(parent, `${name}_${this.sanitizeBranchName(branch)}`);
  }

  private async worktreePathForBranch(branch: string): Promise<string | null> {
    const out = await this.git.raw(['worktree', 'list', '--porcelain']);
    const blocks = out
      .split(/\r?\n\r?\n/)
      .map((b) => b.trim())
      .filter(Boolean);

    for (const block of blocks) {
      let wtPath: string | null = null;
      let wtBranch: string | null = null;
      for (const rawLine of block.split(/\r?\n/)) {
        const line = rawLine.trim();
        if (line.startsWith('worktree ')) wtPath = line.slice('worktree '.length).trim();
        if (line.startsWith('branch ')) wtBranch = line.slice('branch '.length).trim();
      }

      if (!wtPath || !wtBranch) continue;
      const short = wtBranch.replace(/^refs\//, '');
      if (short === branch || short === `heads/${branch}`) {
        return path.resolve(wtPath);
      }
    }

    const fallback = path.resolve(await this.defaultWorktreeDirFromBranch(branch));
    if (fs.existsSync(fallback)) return fallback;
    return null;
  }

  private async switchWorktreeTarget(
    target: string,
    options: { branch?: string; path?: string; print?: boolean; shell?: string },
  ): Promise<void> {
    let resolved: string | null = null;

    if (options.path) {
      resolved = path.resolve(options.path);
    } else if (options.branch) {
      resolved = await this.worktreePathForBranch(options.branch);
    } else {
      const asPath = path.resolve(target);
      if (fs.existsSync(asPath)) {
        resolved = asPath;
      } else {
        resolved = await this.worktreePathForBranch(target);
      }
    }

    if (!resolved) {
      console.error(`Cannot resolve worktree for: ${target}`);
      process.exit(1);
    }

    if (options.print) {
      process.stdout.write(resolved + '\n');
      return;
    }

    const shell = options.shell ?? process.env.SHELL;
    if (!shell) {
      console.error('No shell found. Provide --shell or set $SHELL');
      process.exit(1);
    }

    const child = spawn(shell, {
      cwd: resolved,
      stdio: 'inherit',
      env: process.env,
    });

    child.on('exit', (code) => {
      process.exit(code ?? 0);
    });

    child.on('error', (err) => {
      console.error(String(err));
      process.exit(1);
    });
  }
}
