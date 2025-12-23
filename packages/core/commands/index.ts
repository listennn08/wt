#!/usr/bin/env node

import { Command } from 'commander';
import { SimpleGit } from 'simple-git';
import { AddCommand } from './add';
import { ListCommand } from './list';
import { RemoveCommand } from './remove';
import { CompletionCommand } from './completion';
import { PruneCommand } from './prune';
import { SwitchCommand } from './switch';
import { UninstallCommand } from './uninstall';
import { TuiCommand } from './tui';


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
    new TuiCommand(this.git).load(program);
    new CompletionCommand(this.git).load(program);
    new UninstallCommand(this.git).load(program);
    new PruneCommand(this.git).load(program);
  }
}