#!/usr/bin/env node

import fs from 'fs';
import path from 'path';
import chalk from 'chalk';
import { createRequire } from 'module';
import simpleGit from 'simple-git';
import { Command } from 'commander';
import { CommandLoader } from '../commands';

function findGitRoot(startDir: string): string | null {
  let dir = path.resolve(startDir);
  while (true) {
    const gitPath = path.join(dir, '.git');
    if (fs.existsSync(gitPath)) return dir;

    const parent = path.dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
}

async function bootstrap() {
  const BASE_DIR = findGitRoot(process.cwd()) ?? process.cwd();
  const gitDir = path.join(BASE_DIR, '.git');
  const git = simpleGit(BASE_DIR);

  if (!fs.existsSync(gitDir)) {
    console.error(chalk.dim('No Git repository found in base directory. Run'), chalk.cyan('`git init`'), chalk.dim('first.'));
    process.exit(1);
  }

  const program = new Command();
  const require = createRequire(__filename);
  const pkgJsonPath = path.resolve(__dirname, '..', '..', 'package.json');
  const pkgJson = require(pkgJsonPath);

  program.name('wt')
    .description(pkgJson?.description || '')
    .version(pkgJson?.version || '')
    .usage('<command> [options]')
    .helpOption('-h, --help', 'Display help for command')

  const loader = new CommandLoader(git);
  loader.load(program);
  program.parse();
  // program.outputHelp();
}

bootstrap()
