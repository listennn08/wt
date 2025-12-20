import  { Command } from 'commander';
import { SimpleGit } from 'simple-git';
import chalk from 'chalk';

export abstract class AbstractCommand {
  constructor(protected git: SimpleGit) {}

  public abstract load(program: Command): void;

  protected log(msg: string) {
    process.stdout.write(`${chalk.bgGreen.white('[wt]')} ${msg}\n`);
  }
}