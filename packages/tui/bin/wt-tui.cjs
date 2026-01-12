#!/usr/bin/env node

const path = require('path');
const { spawn } = require('child_process');

function getBinaryPath() {
  const ext = process.platform === 'win32' ? '.exe' : '';
  return path.resolve(__dirname, '..', 'target', 'release', `wt-tui${ext}`);
}

function main() {
  const binPath = getBinaryPath();

  const child = spawn(binPath, process.argv.slice(2), {
    stdio: 'inherit',
  });

  child.on('exit', (code) => {
    process.exit(code ?? 0);
  });

  child.on('error', (err) => {
    process.stderr.write(`[wt-tui] Failed to launch native binary: ${err.message}\n`);
    process.stderr.write('[wt-tui] Try reinstalling to rebuild the binary, or build manually: cargo build --release\n');
    process.exit(1);
  });
}

main();
