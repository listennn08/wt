import 'zx/globals';
import { existsSync } from 'fs';
import path from 'path';

export type PackageManager = 'pnpm' | 'yarn' | 'npm' | 'bun';

export function detectPackageManager(dir: string): PackageManager | null {
  if (existsSync(path.join(dir, 'pnpm-lock.yaml'))) return 'pnpm';
  if (existsSync(path.join(dir, 'yarn.lock'))) return 'yarn';
  if (existsSync(path.join(dir, 'package-lock.json'))) return 'npm';
  if (existsSync(path.join(dir, 'bun.lockb')) || existsSync(path.join(dir, 'bun.lock'))) return 'bun';
  if (existsSync(path.join(dir, 'package.json'))) return 'npm';
  return null;
}

export function dependenciesInstalled(dir: string): boolean {
  return existsSync(path.join(dir, 'node_modules'));
}

export function installCommandString(pm: PackageManager): string {
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

export async function runInstall(pm: PackageManager, cwd: string): Promise<void> {
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
