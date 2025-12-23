import { Box, Text, useApp, useInput } from 'ink';
import fs from 'fs';
import path from 'path';
import React, { useEffect, useMemo, useRef, useState } from 'react';
import type { SimpleGit } from 'simple-git';

import type { WorktreeInfo } from './index.js';

type Mode = 'normal' | 'confirmRemove' | 'promptAdd';

type Focus = 'list' | 'terminal';

type Key = {
  escape?: boolean;
  return?: boolean;
  backspace?: boolean;
  delete?: boolean;
  ctrl?: boolean;
  meta?: boolean;
  tab?: boolean;
  upArrow?: boolean;
  downArrow?: boolean;
  leftArrow?: boolean;
  rightArrow?: boolean;
};

type TerminalRuntime = {
  dispose: () => void;
  write: (data: string) => void;
  snapshotLines: (maxLines: number) => string[];
};

async function createTerminalRuntime(opts: { cwd: string; cols: number; rows: number; shell: string }): Promise<TerminalRuntime> {
  const ptyAny: any = await import('node-pty');
  const xtermAny: any = await import('xterm-headless');

  const ptySpawn: any = ptyAny?.spawn ?? ptyAny?.default?.spawn ?? ptyAny?.default;
  if (typeof ptySpawn !== 'function') {
    throw new Error('node-pty import did not provide a spawn() function');
  }

  const TerminalCtor: any =
    xtermAny?.Terminal ??
    xtermAny?.default?.Terminal ??
    xtermAny?.default ??
    xtermAny;
  if (typeof TerminalCtor !== 'function') {
    throw new Error('xterm-headless import did not provide a Terminal constructor');
  }

  const pty = ptySpawn(opts.shell, [], {
    name: 'xterm-256color',
    cols: opts.cols,
    rows: opts.rows,
    cwd: opts.cwd,
    env: process.env,
  });

  const term = new TerminalCtor({
    cols: opts.cols,
    rows: opts.rows,
    convertEol: true,
    allowProposedApi: true,
  });

  const onData = (data: string) => {
    term.write(data);
  };

  pty.onData(onData);

  return {
    dispose: () => {
      try {
        pty.kill();
      } catch {
        // ignore
      }
      try {
        term.dispose();
      } catch {
        // ignore
      }
    },
    write: (data: string) => {
      pty.write(data);
    },
    snapshotLines: (maxLines: number) => {
      const buf = term.buffer.active;
      const total = buf.length;
      const start = Math.max(0, total - maxLines);
      const lines: string[] = [];
      for (let i = start; i < total; i++) {
        const line = buf.getLine(i);
        if (!line) continue;
        lines.push(line.translateToString(true));
      }
      return lines;
    },
  };
}

function keyToAnsi(input: string, key: Key): string | null {
  if (key.return) return '\r';
  if (key.backspace) return '\x08';
  if (key.delete) return '\x1b[3~';
  if (key.upArrow) return '\x1b[A';
  if (key.downArrow) return '\x1b[B';
  if (key.rightArrow) return '\x1b[C';
  if (key.leftArrow) return '\x1b[D';

  if (key.ctrl && input) {
    const c = input.toLowerCase();
    if (c >= 'a' && c <= 'z') {
      return String.fromCharCode(c.charCodeAt(0) - 96);
    }
  }

  if (input) return input;
  return null;
}

export function App(props: {
  git: SimpleGit;
  fetchWorktrees: (git: SimpleGit) => Promise<{ baseTop: string; worktrees: WorktreeInfo[] }>;
  displayBranchLabel: (wt: WorktreeInfo) => string;
}) {
  const { exit } = useApp();
  const git = props.git;

  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [baseTop, setBaseTop] = useState<string>('');
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([]);
  const [selected, setSelected] = useState(0);
  const [mode, setMode] = useState<Mode>('normal');
  const [message, setMessage] = useState<string | null>(null);
  const [addBranch, setAddBranch] = useState('');
  const [focus, setFocus] = useState<Focus>('list');

  const terminalRef = useRef<TerminalRuntime | null>(null);
  const [terminalLines, setTerminalLines] = useState<string[]>([]);
  const [terminalCwd, setTerminalCwd] = useState<string | null>(null);

  const selectedWt = useMemo(() => worktrees[selected], [worktrees, selected]);

  async function refresh() {
    setLoading(true);
    setError(null);
    try {
      const res = await props.fetchWorktrees(git);
      setBaseTop(res.baseTop);
      setWorktrees(res.worktrees);
      setSelected((prev) => {
        const next = Math.max(0, Math.min(prev, res.worktrees.length - 1));
        return Number.isFinite(next) ? next : 0;
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  async function restartTerminal(dir: string) {
    terminalRef.current?.dispose();
    terminalRef.current = null;
    setTerminalLines([]);
    setTerminalCwd(dir);

    const shell = process.env['SHELL'] || '/bin/bash';
    const cols = Math.max(40, process.stdout.columns || 80);
    const rows = Math.max(10, (process.stdout.rows || 24) - 8);

    try {
      terminalRef.current = await createTerminalRuntime({ cwd: dir, cols, rows, shell });
    } catch (e) {
      setMessage(`Failed to start embedded terminal (node-pty build might be blocked). Error: ${String(e)}`);
      return;
    }
  }

  // Behavior B: restart terminal when selection changes
  useEffect(() => {
    const dir = selectedWt?.path;
    if (!dir) return;
    if (terminalCwd === dir) return;
    void restartTerminal(dir);
  }, [selectedWt?.path]);

  useEffect(() => {
    const id = setInterval(() => {
      const rt = terminalRef.current;
      if (!rt) return;
      const maxLines = Math.max(8, (process.stdout.rows || 24) - 10);
      setTerminalLines(rt.snapshotLines(maxLines));
    }, 50);
    return () => clearInterval(id);
  }, []);

  useEffect(() => {
    return () => {
      terminalRef.current?.dispose();
      terminalRef.current = null;
    };
  }, []);

  async function removeSelected() {
    if (!selectedWt) return;
    try {
      await git.raw(['worktree', 'remove', selectedWt.path]);
      setMessage(`removed: ${selectedWt.path}`);
      await refresh();
    } catch (e) {
      setMessage(`remove failed: ${String(e)}`);
    }
  }

  async function prune() {
    try {
      await git.raw(['worktree', 'prune']);
      setMessage('pruned');
      await refresh();
    } catch (e) {
      setMessage(`prune failed: ${String(e)}`);
    }
  }

  async function addWorktreeFromBranch(branch: string) {
    const b = branch.trim();
    if (!b) {
      setMessage('branch is empty');
      return;
    }

    const top = baseTop;
    const parent = path.dirname(top);
    const name = path.basename(top);
    const sanitized = b.trim().replace(/\s+/g, '-').replace(/[\\/]+/g, '-');
    const dir = path.join(parent, `${name}_${sanitized}`);

    if (fs.existsSync(dir)) {
      setMessage(`target exists: ${dir}`);
      return;
    }

    try {
      await git.raw(['worktree', 'add', dir, b]);
      setMessage(`added: ${dir}`);
      await refresh();
    } catch (e) {
      setMessage(`add failed: ${String(e)}`);
    }
  }

  useInput((input: string, key: Key) => {
    if (key.escape) {
      if (mode !== 'normal') {
        setMode('normal');
        setAddBranch('');
        setMessage(null);
        return;
      }
      exit();
      return;
    }

    if (key.tab) {
      setFocus((f) => (f === 'list' ? 'terminal' : 'list'));
      return;
    }

    if (mode === 'promptAdd') {
      if (key.return) {
        void (async () => {
          const b = addBranch;
          setMode('normal');
          setAddBranch('');
          await addWorktreeFromBranch(b);
        })();
        return;
      }
      if (key.backspace || key.delete) {
        setAddBranch((s) => s.slice(0, -1));
        return;
      }
      if (input && !key.ctrl && !key.meta) {
        setAddBranch((s) => s + input);
        return;
      }
      return;
    }

    if (mode === 'confirmRemove') {
      if (input.toLowerCase() === 'y') {
        setMode('normal');
        void removeSelected();
        return;
      }
      if (input.toLowerCase() === 'n' || key.return) {
        setMode('normal');
        return;
      }
      return;
    }

    if (focus === 'list') {
      if (key.upArrow) {
        setSelected((s) => Math.max(0, s - 1));
        return;
      }
      if (key.downArrow) {
        setSelected((s) => Math.min(worktrees.length - 1, s + 1));
        return;
      }
    }

    if (focus === 'terminal') {
      const rt = terminalRef.current;
      if (rt) {
        const seq = keyToAnsi(input, key);
        if (seq) rt.write(seq);
        return;
      }
    }

    if (key.return) {
      if (focus === 'list') {
        setFocus('terminal');
        return;
      }
      return;
    }

    if (input === 'q') {
      exit();
      return;
    }

    if (input === 'g') {
      void refresh();
      return;
    }

    if (input === 'x') {
      void prune();
      return;
    }

    if (input === 'r') {
      setMode('confirmRemove');
      return;
    }

    if (input === 'a') {
      setMode('promptAdd');
      setAddBranch('');
      return;
    }

    if (input === 'p') {
      if (selectedWt) {
        process.stdout.write(selectedWt.path + '\n');
        process.exit(0);
      }
    }
  });

  if (loading) {
    return (
      <Box flexDirection="column">
        <Text>Loading worktrees…</Text>
      </Box>
    );
  }

  if (error) {
    return (
      <Box flexDirection="column">
        <Text color="red">{error}</Text>
        <Text>Press q to quit.</Text>
      </Box>
    );
  }

  const leftWidth = 50;

  return (
    <Box flexDirection="column" padding={1}>
      <Box>
        <Text bold>wt tui</Text>
        <Text> </Text>
        <Text color="gray">{baseTop}</Text>
      </Box>

      <Box marginTop={1}>
        <Box flexDirection="column" width={leftWidth}>
          <Text bold>Worktrees</Text>
          {worktrees.length === 0 ? (
            <Text color="yellow">(none)</Text>
          ) : (
            worktrees.map((wt, idx) => {
              const isBase = wt.path === baseTop;
              const label = props.displayBranchLabel(wt);
              const head = wt.head ? wt.head.slice(0, 8) : '';
              const flags = [isBase ? 'base' : '', wt.locked ? 'locked' : '', wt.prunable ? 'prunable' : '']
                .filter(Boolean)
                .join(',');

              const selectedRow = idx === selected;
              return (
                <Text key={wt.path} color={selectedRow ? 'cyan' : undefined}>
                  {selectedRow ? '>' : ' '} {label || '(no-branch)'} {head ? `@${head}` : ''} {flags ? `[${flags}]` : ''}
                </Text>
              );
            })
          )}
        </Box>

        <Box flexDirection="column" marginLeft={2} flexGrow={1}>
          <Text bold>
            {focus === 'terminal' ? 'Terminal' : 'Details'}
          </Text>
          {selectedWt ? (
            <>
              <Text>
                <Text color="gray">path: </Text>
                {selectedWt.path}
              </Text>
              <Text>
                <Text color="gray">branch: </Text>
                {props.displayBranchLabel(selectedWt) || '(none)'}
              </Text>
              <Text>
                <Text color="gray">head: </Text>
                {selectedWt.head || '(unknown)'}
              </Text>
              <Text>
                <Text color="gray">flags: </Text>
                {[selectedWt.locked ? 'locked' : '', selectedWt.prunable ? 'prunable' : ''].filter(Boolean).join(',') || '(none)'}
              </Text>
              <Box marginTop={1} flexDirection="column" borderStyle="round" borderColor={focus === 'terminal' ? 'cyan' : 'gray'}>
                {terminalLines.length === 0 ? (
                  <Text color="gray">(terminal starting…)</Text>
                ) : (
                  terminalLines.map((l, i) => <Text key={i}>{l}</Text>)
                )}
              </Box>
            </>
          ) : (
            <Text color="yellow">No selection</Text>
          )}
        </Box>
      </Box>

      <Box marginTop={1} flexDirection="column">
        {mode === 'confirmRemove' ? <Text color="yellow">Remove selected worktree? (y/n)</Text> : null}
        {mode === 'promptAdd' ? <Text color="yellow">Add worktree from branch: {addBranch}</Text> : null}
        {message ? <Text color="gray">{message}</Text> : null}
        <Text color="gray">
          keys: Tab focus | ↑↓ select | Enter focus terminal | a add | r remove | x prune | g refresh | p print path | q quit
        </Text>
      </Box>
    </Box>
  );
}