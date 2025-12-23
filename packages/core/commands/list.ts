import { AbstractCommand } from "./base";
import { SimpleGit } from "simple-git";
import { Command } from "commander";
import { WorktreeInfo } from "../types";
import path from 'path';
import chalk from 'chalk';

export class ListCommand extends AbstractCommand {
  constructor(protected git: SimpleGit) { super(git) }

  public load(program: Command): void {
    program
      .command('list')
      .description('List all worktrees')
      .option('--raw', 'Print raw `git worktree list` output')
      .option('--json', 'Print JSON output')
      .action(async (options: { raw?: boolean; json?: boolean }) => {
        await this.listWorktrees({ raw: options.raw, json: options.json });
      });
  }

  private parseWorktreePorcelain(out: string): WorktreeInfo[] {
    const blocks = out
      .split(/\r?\n\r?\n/)
      .map((b) => b.trim())
      .filter(Boolean);

    const worktrees: WorktreeInfo[] = [];
    for (const block of blocks) {
      const wt: WorktreeInfo = { path: '' };
      for (const rawLine of block.split(/\r?\n/)) {
        const line = rawLine.trim();
        if (!line) continue;

        if (line.startsWith('worktree ')) {
          wt.path = line.slice('worktree '.length).trim();
          continue;
        }
        if (line.startsWith('HEAD ')) {
          wt.head = line.slice('HEAD '.length).trim();
          continue;
        }
        if (line.startsWith('branch ')) {
          const ref = line.slice('branch '.length).trim();
          wt.branch = ref.replace(/^refs\//, '');
          wt.detached = false;
          continue;
        }
        if (line === 'detached') {
          wt.detached = true;
          continue;
        }
        if (line === 'locked') {
          wt.locked = true;
          continue;
        }
        if (line.startsWith('prunable ')) {
          wt.prunable = true;
          continue;
        }
      }

      if (wt.path) worktrees.push(wt);
    }

    return worktrees;
  }

  private displayBranchLabel(wt: WorktreeInfo): string {
    if (wt.branch) return wt.branch;
    if (wt.detached) return 'detached';
    return '';
  }
  
  private pad(s: string, width: number): string {
    if (s.length >= width) return s;
    return s + ' '.repeat(width - s.length);
  }
  
  private async listWorktrees(opts?: { raw?: boolean; json?: boolean }): Promise<void> {
    if (opts?.raw) {
      const out = await this.git.raw(['worktree', 'list']);
      process.stdout.write(out);
      return;
    }
  
    const baseTop = (await this.git.revparse(['--show-toplevel'])).trim();
    const out = await this.git.raw(['worktree', 'list', '--porcelain']);
    const worktrees = this.parseWorktreePorcelain(out)
      .map((wt) => ({
        ...wt,
        path: path.resolve(wt.path),
      }))
      .sort((a, b) => a.path.localeCompare(b.path));
  
    const normalizedBase = path.resolve(baseTop);
  
    if (opts?.json) {
      process.stdout.write(
        JSON.stringify(
          worktrees.map((wt) => ({
            ...wt,
            isBase: wt.path === normalizedBase,
            branchLabel: this.displayBranchLabel(wt),
            shortHead: wt.head ? wt.head.slice(0, 8) : undefined,
          })),
          null,
          2,
        ) + '\n',
      );
      return;
    }
  
    const rows = worktrees.map((wt) => {
      const isBase = wt.path === normalizedBase;
      const branchLabel = this.displayBranchLabel(wt);
      const head = wt.head ? wt.head.slice(0, 8) : '';
      const flags = [isBase ? 'base' : '', wt.locked ? 'locked' : '', wt.prunable ? 'prunable' : '']
        .filter(Boolean)
        .join(',');
      return {
        path: wt.path,
        branch: branchLabel,
        head,
        flags,
        isBase,
      };
    });
  
    const pathWidth = Math.min(
      60,
      Math.max('PATH'.length, ...rows.map((r) => r.path.length)),
    );
    const branchWidth = Math.min(
      30,
      Math.max('BRANCH'.length, ...rows.map((r) => r.branch.length)),
    );
  
    const header = `${this.pad('PATH', pathWidth)}  ${this.pad('BRANCH', branchWidth)}  ${this.pad('HEAD', 8)}  FLAGS`;
    process.stdout.write(chalk.bold(chalk.gray(header)) + '\n');
  
    for (const r of rows) {
      const p = r.path.length > pathWidth ? `…${r.path.slice(-(pathWidth - 1))}` : r.path;
  
      const pathCol = chalk.dim(this.pad(p, pathWidth));
      const branchCol = r.branch ? chalk.cyan(this.pad(r.branch, branchWidth)) : this.pad('', branchWidth);
      const headCol = r.head ? chalk.gray(this.pad(r.head, 8)) : this.pad('', 8);
  
      const flagsRaw = r.flags
        .split(',')
        .map((f) => f.trim())
        .filter(Boolean);
      const flagsCol = flagsRaw
        .map((f) => {
          if (f === 'base') return chalk.green(f);
          if (f === 'locked') return chalk.yellow(f);
          if (f === 'prunable') return chalk.red(f);
          return chalk.gray(f);
        })
        .join(chalk.gray(','));
  
      const line = `${pathCol}  ${branchCol}  ${headCol}  ${flagsCol}`;
      process.stdout.write(r.isBase ? chalk.bold(line) + '\n' : line + '\n');
    }
  }
}