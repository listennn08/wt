#!/usr/bin/env node

import { Command } from 'commander';
import { SimpleGit } from 'simple-git';
import { AddCommand } from './add.js';
import { ListCommand } from './list.js';
import { RemoveCommand } from './remove.js';
import { CompletionCommand } from './completion.js';
import { PruneCommand } from './prune.js';
import { SwitchCommand } from './switch.js';
import { UninstallCommand } from './uninstall.js';


export class CommandLoader {
  private git: SimpleGit;

  constructor(git: SimpleGit) {
    this.git = git;
  }

  load(program: Command) {
    new AddCommand(this.git).load(program);
    new ListCommand(this.git).load(program);
    new RemoveCommand(this.git).load(program);
    new SwitchCommand(this.git).load(program);
    new CompletionCommand(this.git).load(program);
    new UninstallCommand(this.git).load(program);
    new PruneCommand(this.git).load(program);
  }
}