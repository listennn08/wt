import { Command } from "commander";
import fs from 'fs';
import path from 'path';
import chalk from 'chalk';
import os from 'os';
import { spawn } from 'child_process';
import { SimpleGit } from "simple-git";
import * as toml from '@iarna/toml';
import  { AbstractCommand } from "./base";

type HookCommand = {
  program: string;
  args?: string[];
  cwd?: string;
};

type WtConfig = {
  hooks?: {
    add?: {
      pre_create?: HookCommand[];
      post_create?: HookCommand[];
      disable_default_post_create?: boolean;
    };
  };
};

export class AddCommand extends AbstractCommand {
  constructor(protected git: SimpleGit) { super(git); }

  private readConfigIfExists(filePath: string): WtConfig | null {
    if (!fs.existsSync(filePath)) return null;
    const content = fs.readFileSync(filePath, 'utf8');
    try {
      return toml.parse(content) as unknown as WtConfig;
    } catch (err: any) {
      throw new Error(`Failed to parse config ${filePath}: ${err?.message ?? String(err)}`);
    }
  }

  private loadConfig(baseTop: string): WtConfig {
    const candidates: string[] = [
      path.join(baseTop, '.wt.toml'),
      path.join(baseTop, 'wt.toml'),
      path.join(baseTop, '.config', 'wt', 'config.toml'),
    ];

    const xdg = process.env.XDG_CONFIG_HOME;
    if (xdg) {
      candidates.push(path.join(xdg, 'wt', 'config.toml'));
    } else {
      candidates.push(path.join(os.homedir(), '.config', 'wt', 'config.toml'));
    }

    for (const p of candidates) {
      const cfg = this.readConfigIfExists(p);
      if (cfg) return cfg;
    }
    return {};
  }

  private async runHookCommands(
    hookName: string,
    commands: HookCommand[] | undefined,
    ctx: { baseTop: string; worktreePath: string; branch: string },
  ): Promise<void> {
    if (!commands || commands.length === 0) return;

    for (const cmd of commands) {
      const args = cmd.args ?? [];

      const cwd = (() => {
        if (!cmd.cwd) return ctx.baseTop;
        if (cmd.cwd === '${base}') return ctx.baseTop;
        if (cmd.cwd === '${worktree}') return ctx.worktreePath;
        return cmd.cwd;
      })();

      await new Promise<void>((resolve, reject) => {
        const child = spawn(cmd.program, args, {
          cwd,
          stdio: 'inherit',
          env: {
            ...process.env,
            WT_BASE: ctx.baseTop,
            WT_WORKTREE: ctx.worktreePath,
            WT_BRANCH: ctx.branch,
          },
        });

        child.on('error', (err) => {
          reject(new Error(`Hook ${hookName} failed to start: ${cmd.program}: ${err.message}`));
        });

        child.on('exit', (code) => {
          if (code === 0) return resolve();
          const cmdline = [cmd.program, ...args].join(' ');
          reject(new Error(`Hook ${hookName} failed: cmd='${cmdline}' cwd='${cwd}' exitCode=${code ?? 'null'}`));
        });
      });
    }
  }

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

    const cfg = this.loadConfig(baseTop);
    const addHooks = cfg.hooks?.add;
    await this.runHookCommands('hooks.add.pre_create', addHooks?.pre_create, {
      baseTop,
      worktreePath,
      branch,
    });
  
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
      if (!addHooks?.disable_default_post_create) {
        await this.postCreate(baseTop, worktreePath, opts);
      }
      await this.runHookCommands('hooks.add.post_create', addHooks?.post_create, {
        baseTop,
        worktreePath,
        branch,
      });
      this.log(worktreePath);
      return;
    }
  
    if (remoteExists && !opts.newBranch) {
      this.log(`worktree add (from remote ${remote}/${branch}): ${branch}`);
      await this.git.raw(['worktree', 'add', '-b', branch, worktreePath, `${remote}/${branch}`]);
      if (!addHooks?.disable_default_post_create) {
        await this.postCreate(baseTop, worktreePath, opts);
      }
      await this.runHookCommands('hooks.add.post_create', addHooks?.post_create, {
        baseTop,
        worktreePath,
        branch,
      });
      this.log(worktreePath);
      return;
    }
  
    const currentBranch = (await this.git.raw(['branch', '--show-current'])).trim();
    const base = opts.base ?? (currentBranch || 'HEAD');
    this.log(`worktree add (new branch from ${base}): ${branch}`);
    await this.git.raw(['worktree', 'add', '-b', branch, worktreePath, base]);
    if (!addHooks?.disable_default_post_create) {
      await this.postCreate(baseTop, worktreePath, opts);
    }
    await this.runHookCommands('hooks.add.post_create', addHooks?.post_create, {
      baseTop,
      worktreePath,
      branch,
    });
    this.log(worktreePath);
  }

  private async postCreate(baseTop: string, worktreePath: string, opts: { install?: boolean }) {
    this.copyEnvFiles(baseTop, worktreePath);
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