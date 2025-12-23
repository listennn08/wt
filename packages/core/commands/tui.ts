import { Command } from 'commander';
import { spawn } from 'child_process';
import { createRequire } from 'module';
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
    const require = createRequire(__filename);
    const cliPath = require.resolve('@wt/tui/dist/cli.js');

    const child = spawn(process.execPath, [cliPath], {
      cwd: process.cwd(),
      stdio: 'inherit',
      env: {
        ...process.env,
        WT_BASE_CWD: process.cwd(),
      },
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
