import { Command } from "commander";
import fs from 'fs';
import path from 'path';
import chalk from 'chalk';
import { SimpleGit } from "simple-git";
import { dependenciesInstalled, detectPackageManager, installCommandString, runInstall } from "../utils/packageManager";
import  { AbstractCommand } from "./base";

export class AddCommand extends AbstractCommand {
  constructor(protected git: SimpleGit) { super(git); }

  public load(program: Command): void {
    program
      .command('add')
      .description('Add a new worktree from a branch')
      .argument('<branch>', 'Branch to create/use for worktree')
      .option('-d, --dir <path>', 'Target directory for the worktree')
      .option('-n, --new-branch', 'Force creating a new branch even if remote branch exists')
      .option('-b, --base <ref>', 'Base ref when creating a new branch (default: current branch)')
      .option('-r, --remote <name>', 'Remote name (default: origin)', 'origin')
      .option('-f, --force', 'Allow if target directory already exists')
      .option('--no-install', 'Do not run package install in the new worktree')
      .option('--no-progress', 'Do not print step/progress messages')
      .action(async (
        branch: string,
        options: {
          dir?: string;
          newBranch?: boolean;
          base?: string;
          remote?: string;
          force?: boolean;
          install?: boolean;
          progress?: boolean;
        },
      ) => {
        await this.addWorktree({
          branch,
          dir: options.dir,
          newBranch: options.newBranch,
          base: options.base,
          remote: options.remote,
          force: options.force,
          install: options.install,
          progress: options.progress,
        });
      });
  }

  private async addWorktree(opts: {
    branch: string;
    dir?: string;
    newBranch?: boolean;
    base?: string;
    remote?: string;
    force?: boolean;
    install?: boolean;
    progress?: boolean;
  }) {
    const branch = opts.branch;
    const remote = opts.remote ?? 'origin';
    const baseTop = (await this.git.revparse(['--show-toplevel'])).trim();
    const worktreePath = opts.dir ? path.resolve(opts.dir) : path.resolve(await this.defaultWorktreeDir(branch));
  
    if (fs.existsSync(worktreePath) && !opts.force) {
      console.warn(chalk.yellow`Target path already exists: ` + chalk.yellow(worktreePath));
      console.warn(chalk.yellow`Use --force to proceed (git may still fail if it is not a worktree directory).`);
      process.exit(1);
    }
  
    const localExists = await this.branchExistsLocal(branch);
    const remoteExists = await this.branchExistsRemote(remote, branch);
  
    if (localExists) {
      this.log(`worktree add (existing local branch): ${branch}`);
      await this.git.raw(['worktree', 'add', worktreePath, branch]);
      await this.postCreate(baseTop, worktreePath, opts);
      this.log(worktreePath);
      return;
    }
  
    if (remoteExists && !opts.newBranch) {
      this.log(`worktree add (from remote ${remote}/${branch}): ${branch}`);
      await this.git.raw(['worktree', 'add', '-b', branch, worktreePath, `${remote}/${branch}`]);
      await this.postCreate(baseTop, worktreePath, opts);
      this.log(worktreePath);
      return;
    }
  
    const currentBranch = (await this.git.raw(['branch', '--show-current'])).trim();
    const base = opts.base ?? (currentBranch || 'HEAD');
    this.log(`worktree add (new branch from ${base}): ${branch}`);
    await this.git.raw(['worktree', 'add', '-b', branch, worktreePath, base]);
    await this.postCreate(baseTop, worktreePath, opts);
    this.log(worktreePath);
  }

  private async postCreate(baseTop: string, worktreePath: string, opts: { install?: boolean }) {
    this.copyEnvFiles(baseTop, worktreePath);

    const pm = detectPackageManager(worktreePath);
    if (!pm) return;

    if (dependenciesInstalled(worktreePath)) return;

    const cmd = installCommandString(pm);
    if (opts.install !== false) {
      this.log(`Installing deps: ${cmd}`);
      await runInstall(pm, worktreePath);
      this.log(`deps installed`);
      return;
    }

    console.info(chalk.red`Dependencies not installed in: ${worktreePath}`);
    console.info(chalk.yellow`Run: (cd ${worktreePath} && ${cmd})`);
  }

  private async repoName(): Promise<string> {
    const top = (await this.git.revparse(['--show-toplevel'])).trim();
    return path.basename(top);
  }
    
  private async defaultWorktreeDir(branch: string): Promise<string> {
    const top = (await this.git.revparse(['--show-toplevel'])).trim();
    const parent = path.dirname(top);
    const name = await this.repoName();
    return path.join(parent, `${name}_${this.sanitizeBranchName(branch)}`);
  }
    
  private async branchExistsLocal(branch: string): Promise<boolean> {
    try {
      await this.git.revparse(['--verify', branch]);
      return true;
    } catch {
      return false;
    }
  }

  private sanitizeBranchName(branch: string): string {
    return branch.trim().replace(/\s+/g, '-').replace(/[\\/]+/g, '-');
  }

  private async branchExistsRemote(remote: string, branch: string): Promise<boolean> {
    try {
      const out = await this.git.raw(['ls-remote', '--heads', remote, branch]);
      return out.trim().length > 0;
    } catch {
      return false;
    }
  }

  private copyEnvFiles(baseTop: string, worktreePath: string): void {
    const candidates = ['.env', '.env.local'];
    for (const filename of candidates) {
      const src = path.join(baseTop, filename);
      const dst = path.join(worktreePath, filename);
      if (!fs.existsSync(src)) continue;
      if (fs.existsSync(dst)) continue;
      try {
        fs.copyFileSync(src, dst);
        this.log(`copied ${chalk.rgb(213, 142, 67)(filename)}`);
      } catch (e) {
        this.log(chalk.red`failed to copy ${filename}`);
      }
    }
  }
}