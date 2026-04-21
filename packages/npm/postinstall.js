#!/usr/bin/env node
const { execSync } = require("child_process");
const fs = require("fs");
const os = require("os");
const path = require("path");
const https = require("https");

const REPO = "listennn08/wt";
const BIN_DIR = path.join(__dirname, "bin");

function getPlatformTarget() {
  const platform = os.platform();
  const arch = os.arch();
  const map = {
    "darwin-x64": "x86_64-apple-darwin",
    "darwin-arm64": "aarch64-apple-darwin",
    "linux-x64": "x86_64-unknown-linux-gnu",
    "linux-arm64": "aarch64-unknown-linux-gnu",
  };
  return map[`${platform}-${arch}`];
}

async function download(url, dest) {
  return new Promise((resolve, reject) => {
    const follow = (url) => {
      https.get(url, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          follow(res.headers.location);
          return;
        }
        if (res.statusCode !== 200) {
          reject(new Error(`Download failed: HTTP ${res.statusCode}`));
          return;
        }
        const file = fs.createWriteStream(dest);
        res.pipe(file);
        file.on("finish", () => { file.close(); resolve(); });
      }).on("error", reject);
    };
    follow(url);
  });
}

async function main() {
  const target = getPlatformTarget();
  if (!target) {
    console.error(`Unsupported platform: ${os.platform()}-${os.arch()}`);
    process.exit(1);
  }

  const pkg = require("./package.json");
  const version = pkg.version;
  const assetName = `wt-${target}.tar.gz`;
  const url = `https://github.com/${REPO}/releases/download/v${version}/${assetName}`;

  fs.mkdirSync(BIN_DIR, { recursive: true });
  const tarball = path.join(BIN_DIR, assetName);

  console.log(`Downloading wt v${version} for ${target}...`);
  await download(url, tarball);
  execSync(`tar -xzf "${tarball}" -C "${BIN_DIR}"`, { stdio: "inherit" });
  fs.unlinkSync(tarball);
  fs.chmodSync(path.join(BIN_DIR, "wt"), 0o755);
  console.log("wt installed successfully");
}

main().catch((err) => {
  console.error("Failed to install wt:", err.message);
  process.exit(1);
});
