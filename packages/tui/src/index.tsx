import { render } from 'ink';
import path from 'path';
import { simpleGit, type SimpleGit } from 'simple-git';
import React from 'react';

import { App } from './App.js';

export type WorktreeInfo = {
  path: string;
  head?: string;
  branch?: string;
  detached?: boolean;
  locked?: boolean;
  prunable?: boolean;
};

function parseWorktreePorcelain(out: string): WorktreeInfo[] {
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

function displayBranchLabel(wt: WorktreeInfo): string {
  if (wt.branch) return wt.branch;
  if (wt.detached) return 'detached';
  return '';
}

async function fetchWorktrees(git: SimpleGit): Promise<{ baseTop: string; worktrees: WorktreeInfo[] }> {
  const baseTop = (await git.revparse(['--show-toplevel'])).trim();
  const out = await git.raw(['worktree', 'list', '--porcelain']);

  const worktrees = parseWorktreePorcelain(out)
    .map((wt) => ({
      ...wt,
      path: path.resolve(wt.path),
    }))
    .sort((a, b) => a.path.localeCompare(b.path));

  return { baseTop: path.resolve(baseTop), worktrees };
}

export async function runTui(opts?: { cwd?: string }): Promise<void> {
  const cwd = opts?.cwd ?? process.cwd();
  const git = simpleGit(cwd);

  render(
    <App
      git={git}
      fetchWorktrees={fetchWorktrees}
      displayBranchLabel={displayBranchLabel}
    />,
  );
}
