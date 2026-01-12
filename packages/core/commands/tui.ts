import { Command } from 'commander';
import { spawn } from 'child_process';
import path from 'path';
import { SimpleGit } from 'simple-git';
import { AbstractCommand } from './base';

export class TuiCommand extends AbstractCommand {
  constructor(protected git: SimpleGit) { super(git); }

  public load(program: Command): void {
    program
      .command('tui')
      .description('Interactive TUI for worktrees')
      .action(async () => {
        await this.runTui();
      });
  }

  private async runTui(): Promise<void> {
    const repoRustTuiPath = path.resolve(__dirname, '..', '..', '..', 'tui', 'target', 'release', 'wt-tui');

    let repoRoot = process.cwd();
    try {
      repoRoot = (await this.git.revparse(['--show-toplevel'])).trim();
    } catch (err) {
      // fall back to current directory
    }

    const args = ['--repo', repoRoot];
    const child = spawn('wt-tui', args, {
      cwd: repoRoot,
      stdio: 'inherit',
    });

    child.on('exit', (code) => {
      process.exit(code ?? 0);
    });

    child.on('error', (err) => {
      if ((err as NodeJS.ErrnoException)?.code !== 'ENOENT') {
        console.error(`Failed to launch TUI: ${err.message}`);
        process.exit(1);
      }

      const fallback = spawn(repoRustTuiPath, args, {
        cwd: repoRoot,
        stdio: 'inherit',
      });

      fallback.on('exit', (code) => {
        process.exit(code ?? 0);
      });

      fallback.on('error', (fallbackErr) => {
        console.error(`Failed to launch TUI: ${fallbackErr.message}`);
        console.error('Make sure the Rust TUI is installed or built: npm i -g @listennn08/wt-tui OR cd packages/tui && cargo build --release');
        process.exit(1);
      });
    });
  }
}
