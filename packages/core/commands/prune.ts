import { Command } from 'commander';
import { SimpleGit } from 'simple-git';
import { AbstractCommand } from './base';

export class PruneCommand extends AbstractCommand {
  constructor(protected git: SimpleGit) {
    super(git);
  }

  public load(program: Command): void {
    program
      .command('prune')
      .description('Prune stale worktree information')
      .option('--dry-run', 'Do not remove anything; show what would be pruned')
      .option('--verbose', 'Report all removals')
      .option('--expire <time>', 'Expire worktrees older than <time>')
      .action(async (options: { dryRun?: boolean; verbose?: boolean; expire?: string }) => {
        await this.pruneWorktrees(options);
      });
  }

  private async pruneWorktrees(options: { dryRun?: boolean; verbose?: boolean; expire?: string }): Promise<void> {
    const args = ['worktree', 'prune'];
    if (options.dryRun) args.push('--dry-run');
    if (options.verbose) args.push('--verbose');
    if (options.expire) args.push(`--expire=${options.expire}`);

    const out = await this.git.raw(args);
    if (out) process.stdout.write(out);
  }
}
