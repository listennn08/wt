import { Command } from 'commander';
import { spawn } from 'child_process';
import { createRequire } from 'module';
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
    const rustTuiPath = path.resolve(__dirname, '..', '..', '..', 'tui-rust', 'target', 'release', 'wt-tui');

    let repoRoot = process.cwd();
    try {
      repoRoot = (await this.git.revparse(['--show-toplevel'])).trim();
    } catch (err) {
      // fall back to current directory
    }

    const child = spawn(rustTuiPath, ['--repo', repoRoot], {
      cwd: repoRoot,
      stdio: 'inherit',
    });

    child.on('exit', (code) => {
      process.exit(code ?? 0);
    });

    child.on('error', (err) => {
      console.error(`Failed to launch TUI: ${err.message}`);
      console.error('Make sure the Rust TUI is built: cd packages/tui-rust && cargo build --release');
      process.exit(1);
    });
  }
}
