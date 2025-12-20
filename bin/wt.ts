#!/usr/bin/env node

import fs from 'fs';
import path from 'path';
import chalk from 'chalk';
import pkg from 'local-pkg'
import simpleGit from 'simple-git';
import { Command } from 'commander';
import { CommandLoader } from '../commands';

async function bootstrap() {
  const BASE_DIR = process.cwd();
  const gitDir = path.join(BASE_DIR, '.git');
  const git = simpleGit(BASE_DIR);

  if (!fs.existsSync(gitDir)) {
    console.error(chalk.dim('No Git repository found in base directory. Run'), chalk.cyan('`git init`'), chalk.dim('first.'));
    process.exit(1);
  }

  const program = new Command();
  const pkgJson = pkg.loadPackageJSONSync()

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
