#!/usr/bin/env node

import 'zx/globals';
import { Command } from 'commander';
import { copyFileSync, existsSync } from 'fs';
import path from 'path';
import simpleGit from 'simple-git';

const BASE_DIR = process.cwd();
const gitDir = path.join(BASE_DIR, '.git');
const git = simpleGit(BASE_DIR);
const program = new Command();

program.name('wt').description('Worktree manager').version('1.0.0');

if (!existsSync(gitDir)) {
  console.error('No Git repository found in base directory');
  process.exit(1);
}

function sanitizeBranchName(branch: string): string {
  return branch.trim().replace(/\s+/g, '-').replace(/[\\/]+/g, '-');
}

async function repoName(): Promise<string> {
  const top = (await git.revparse(['--show-toplevel'])).trim();
  return path.basename(top);
}

async function defaultWorktreeDir(branch: string): Promise<string> {
  const top = (await git.revparse(['--show-toplevel'])).trim();
  const parent = path.dirname(top);
  const name = await repoName();
  return path.join(parent, `${name}_${sanitizeBranchName(branch)}`);
}

async function branchExistsLocal(branch: string): Promise<boolean> {
  try {
    await git.revparse(['--verify', branch]);
    return true;
  } catch {
    return false;
  }
}

async function branchExistsRemote(remote: string, branch: string): Promise<boolean> {
  try {
    const out = await git.raw(['ls-remote', '--heads', remote, branch]);
    return out.trim().length > 0;
  } catch {
    return false;
  }
}

async function addWorktree(opts: {
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
  const baseTop = (await git.revparse(['--show-toplevel'])).trim();
  const worktreePath = opts.dir ? path.resolve(opts.dir) : path.resolve(await defaultWorktreeDir(branch));

  const progress = opts.progress !== false;
  const step = (msg: string) => {
    if (progress) process.stdout.write(`${chalk.bgGreen.white('[wt]')} ${msg}\n`);
  };

  async function postCreate() {
    copyEnvFiles(baseTop, worktreePath, step);

    const pm = detectPackageManager(worktreePath);
    if (!pm) return;

    if (dependenciesInstalled(worktreePath)) return;

    const cmd = installCommandString(pm);
    if (opts.install !== false) {
      step(`Installing deps: ${cmd}`);
      await runInstall(pm, worktreePath);
      step(`deps installed`);
      return;
    }

    console.info(chalk.red`Dependencies not installed in: ${worktreePath}`);
    console.info(chalk.yellow`Run: (cd ${worktreePath} && ${cmd})`);
  }

  if (existsSync(worktreePath) && !opts.force) {
    console.warn(chalk.yellow`Target path already exists: ${worktreePath}`);
    console.warn(chalk.yellow`Use --force to proceed (git may still fail if it is not a worktree directory).`);
    process.exit(1);
  }

  const localExists = await branchExistsLocal(branch);
  const remoteExists = await branchExistsRemote(remote, branch);

  if (localExists) {
    step(`worktree add (existing local branch): ${branch}`);
    await git.raw(['worktree', 'add', worktreePath, branch]);
    await postCreate();
    console.log(worktreePath);
    return;
  }

  if (remoteExists && !opts.newBranch) {
    step(`worktree add (from remote ${remote}/${branch}): ${branch}`);
    await git.raw(['worktree', 'add', '-b', branch, worktreePath, `${remote}/${branch}`]);
    await postCreate();
    console.log(worktreePath);
    return;
  }

  const currentBranch = (await git.raw(['branch', '--show-current'])).trim();
  const base = opts.base ?? (currentBranch || 'HEAD');
  step(`worktree add (new branch from ${base}): ${branch}`);
  await git.raw(['worktree', 'add', '-b', branch, worktreePath, base]);
  await postCreate();
  console.log(worktreePath);
}

type PackageManager = 'pnpm' | 'yarn' | 'npm' | 'bun';

function detectPackageManager(dir: string): PackageManager | null {
  if (existsSync(path.join(dir, 'pnpm-lock.yaml'))) return 'pnpm';
  if (existsSync(path.join(dir, 'yarn.lock'))) return 'yarn';
  if (existsSync(path.join(dir, 'package-lock.json'))) return 'npm';
  if (existsSync(path.join(dir, 'bun.lockb')) || existsSync(path.join(dir, 'bun.lock'))) return 'bun';
  if (existsSync(path.join(dir, 'package.json'))) return 'npm';
  return null;
}

function copyEnvFiles(baseTop: string, worktreePath: string, step: (msg: string) => void): void {
  const candidates = ['.env', '.env.local'];
  for (const filename of candidates) {
    const src = path.join(baseTop, filename);
    const dst = path.join(worktreePath, filename);
    if (!existsSync(src)) continue;
    if (existsSync(dst)) continue;
    try {
      copyFileSync(src, dst);
      step(`copied ${chalk.rgb(213, 142, 67)(filename)}`);
    } catch (e) {
      step(chalk.red`failed to copy ${filename}`);
    }
  }
}

function dependenciesInstalled(dir: string): boolean {
  return existsSync(path.join(dir, 'node_modules'));
}

function installCommandString(pm: PackageManager): string {
  switch (pm) {
    case 'pnpm':
      return 'pnpm install';
    case 'yarn':
      return 'yarn install';
    case 'bun':
      return 'bun install';
    case 'npm':
    default:
      return 'npm install';
  }
}

async function runInstall(pm: PackageManager, cwd: string): Promise<void> {
  switch (pm) {
    case 'pnpm':
      await $({ cwd })`pnpm install`;
      return;
    case 'yarn':
      await $({ cwd })`yarn install`;
      return;
    case 'bun':
      await $({ cwd })`bun install`;
      return;
    case 'npm':
    default:
      await $({ cwd })`npm install`;
      return;
  }
}

async function listWorktrees() {
  const out = await git.raw(['worktree', 'list']);
  process.stdout.write(out);
}

async function removeWorktree(worktreePath: string, force?: boolean) {
  const abs = path.resolve(worktreePath);
  await git.raw(force ? ['worktree', 'remove', '--force', abs] : ['worktree', 'remove', abs]);
  console.log(abs);
}

async function branchesForCompletion(): Promise<string[]> {
  const out = await git.raw([
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

async function worktreesForCompletion(): Promise<string[]> {
  const baseTop = (await git.revparse(['--show-toplevel'])).trim();
  const out = await git.raw(['worktree', 'list', '--porcelain']);

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
    await addWorktree({
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

program
  .command('list')
  .description('List all worktrees')
  .action(async () => {
    await listWorktrees();
  });

program
  .command('remove')
  .description('Delete a worktree')
  .argument('<path>', 'Worktree path')
  .option('-f, --force', 'Force removal')
  .action(async (p: string, options: { force?: boolean }) => {
    await removeWorktree(p, options.force);
  });

program
  .command('__complete-branches', { hidden: true })
  .description('Print branches for shell completion')
  .action(async () => {
    const branches = await branchesForCompletion();
    process.stdout.write(branches.join('\n'));
    if (branches.length > 0) process.stdout.write('\n');
  });

program
  .command('__complete-worktrees', { hidden: true })
  .description('Print worktree paths for shell completion')
  .action(async () => {
    const worktrees = await worktreesForCompletion();
    process.stdout.write(worktrees.join('\n'));
    if (worktrees.length > 0) process.stdout.write('\n');
  });

program.parse();