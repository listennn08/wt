const { spawnSync } = require('child_process');

function run(cmd, args, opts = {}) {
  const res = spawnSync(cmd, args, {
    stdio: 'inherit',
    ...opts,
  });
  if (res.status !== 0) {
    process.exit(res.status ?? 1);
  }
}

function main() {
  run('cargo', ['build', '--release'], { cwd: __dirname + '/..' });
}

main();
