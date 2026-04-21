# wt — Full Rust Rewrite Design Spec

## Goal

Rewrite the `wt` git worktree manager from a TypeScript CLI + Rust TUI monorepo into a single Rust binary. Remove the MCP server package entirely.

## Decisions

- **Single binary**: CLI commands (`add`, `list`, `remove`, `switch`, `prune`, `uninstall`, `completion`) and TUI all ship as one `wt` binary
- **MCP server removed**: AI tools can call the CLI directly; `packages/mcp/` is deleted
- **Incremental migration**: Build on top of existing `packages/tui/` Rust code (approach A)
- **Shell completion**: `clap_complete` for base framework + hand-written dynamic completion scripts (branch/worktree lookup via hidden subcommands)
- **Distribution**: crates.io + GitHub Releases prebuilt binaries + npm wrapper package

## Repository Structure

```
/
├── Cargo.toml              # workspace root, bin crate lives here
├── crates/
│   ├── wt-core/            # lib crate — git, config, hooks, env, worktree orchestration
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── git.rs      # git2 operations (expanded from tui/src/git.rs)
│   │   │   ├── config.rs   # TOML config parsing & resolution
│   │   │   ├── hooks.rs    # pre_create / post_create hook execution
│   │   │   ├── worktree.rs # add/remove/list/prune/switch orchestration
│   │   │   └── env.rs      # .env file copying
│   │   └── Cargo.toml
│   └── wt-tui/             # lib crate — TUI rendering & state
│       ├── src/
│       │   ├── lib.rs
│       │   ├── app.rs      # app state, modals, focus management
│       │   ├── ui.rs       # ratatui rendering
│       │   └── terminal.rs # PTY session management
│       └── Cargo.toml
├── src/                    # bin crate
│   ├── main.rs             # clap CLI definition & routing
│   ├── cmd/
│   │   ├── mod.rs
│   │   ├── add.rs
│   │   ├── list.rs
│   │   ├── remove.rs
│   │   ├── switch.rs
│   │   ├── tui.rs
│   │   ├── prune.rs
│   │   ├── completion.rs
│   │   └── uninstall.rs
│   └── output.rs           # [wt] prefix, colored output helpers
├── packages/
│   └── npm/                # lightweight npm wrapper (downloads prebuilt binary)
│       ├── package.json
│       └── postinstall.js
├── .github/
│   └── workflows/
│       └── release.yml     # CI + cross-platform release builds
└── .wt.toml
```

Removed: `packages/core/`, `packages/mcp/`, `pnpm-workspace.yaml` (no longer a pnpm workspace).

## Dependencies

| Purpose | Crate | Notes |
|---------|-------|-------|
| CLI framework | `clap` (derive) + `clap_complete` | |
| Git | `git2` | Already used in TUI |
| TOML config | `toml` + `serde` | Replaces `@iarna/toml` |
| Terminal color | `colored` | Replaces `chalk` |
| Hook execution | `std::process::Command` | Synchronous, replaces Node `spawn` |
| TUI | `ratatui` + `crossterm` | Unchanged |
| PTY | `portable-pty` | Unchanged |
| Async runtime | `tokio` | TUI only; CLI commands are synchronous |
| Error handling | `anyhow` (bin/tui) + `thiserror` (core) | |
| JSON output | `serde_json` | For `wt list --json` |
| Home/XDG paths | `dirs` | |

## Core Module Design

### `wt-core::config`

```rust
#[derive(Deserialize, Default)]
pub struct WtConfig {
    pub hooks: Option<HooksConfig>,
}

#[derive(Deserialize)]
pub struct HooksConfig {
    pub add: Option<AddHooks>,
}

#[derive(Deserialize)]
pub struct AddHooks {
    pub pre_create: Option<Vec<HookCommand>>,
    pub post_create: Option<Vec<HookCommand>>,
    pub disable_default_post_create: Option<bool>,
}

#[derive(Deserialize)]
pub struct HookCommand {
    pub program: String,
    pub args: Option<Vec<String>>,
    pub cwd: Option<String>,
}
```

`load_config(repo_root: &Path) -> WtConfig` searches in order:
1. `{repo}/.wt.toml`
2. `{repo}/wt.toml`
3. `{repo}/.config/wt/config.toml`
4. `~/.config/wt/config.toml`
5. `$XDG_CONFIG_HOME/wt/config.toml`

First match wins. No file found returns `WtConfig::default()`.

### `wt-core::hooks`

`run_hooks(name: &str, commands: &[HookCommand], ctx: &HookContext) -> Result<()>`

- `HookContext` contains `base_top`, `worktree_path`, `branch`
- Substitutes `${base}` and `${worktree}` in `cwd` field
- Injects `WT_BASE`, `WT_WORKTREE`, `WT_BRANCH` as env vars
- Runs each command synchronously via `std::process::Command`
- Non-zero exit code returns an error

### `wt-core::git`

Extends existing `tui/src/git.rs` with:

- `add_worktree(branch, dir, opts)` — three code paths: existing local branch, remote branch tracking, new branch from base ref
- `remove_worktree(target, force)` — resolves target as path or branch name
- `switch_worktree(target)` — resolves target to worktree path
- `prune_worktrees(dry_run, verbose, expire)`
- `list_branches()` — for shell completion
- `list_worktree_paths()` — for shell completion (excludes base worktree)

### `wt-core::env`

`copy_env_files(base: &Path, worktree: &Path)` — copies `.env` and `.env.local` if source exists and destination does not.

### `wt-core::worktree` (orchestration)

```rust
pub struct AddOptions {
    pub branch: String,
    pub dir: Option<PathBuf>,
    pub new_branch: bool,
    pub base: Option<String>,
    pub remote: String,       // default "origin"
    pub force: bool,
    pub install: bool,
    pub progress: bool,
}

pub fn add_worktree(repo: &GitRepo, opts: AddOptions) -> Result<PathBuf>;
pub fn remove_worktree(repo: &GitRepo, target: &str, force: bool) -> Result<PathBuf>;
pub fn list_worktrees(repo: &GitRepo) -> Result<Vec<WorktreeInfo>>;
pub fn prune_worktrees(repo: &GitRepo, opts: PruneOptions) -> Result<()>;
```

This layer orchestrates git + config + hooks + env. Both CLI commands and TUI call into this layer — no duplicated git logic.

## CLI Design

```rust
#[derive(Parser)]
#[command(name = "wt", about = "Git worktree manager")]
enum Cli {
    Add(AddArgs),
    List(ListArgs),
    #[command(alias = "rm")]
    Remove(RemoveArgs),
    Switch(SwitchArgs),
    Tui(TuiArgs),
    Prune(PruneArgs),
    Completion(CompletionArgs),
    Uninstall(UninstallArgs),
    #[command(hide = true)]
    __CompleteBranches,
    #[command(hide = true)]
    __CompleteWorktrees,
    #[command(hide = true)]
    __CompleteActions,
}
```

- CLI commands are synchronous (no tokio runtime)
- `wt tui` initializes tokio runtime and launches the TUI
- All output goes through `output.rs` for consistent `[wt]` green prefix formatting

### Output Formats

- `wt list` — human-readable table (default)
- `wt list --json` — `serde_json` serialized `Vec<WorktreeInfo>`
- `wt list --raw` — passthrough of `git worktree list`

## Shell Completion

**Static**: `clap_complete` generates base completion framework.

**Dynamic**: Three hidden subcommands (`__complete-branches`, `__complete-worktrees`, `__complete-actions`) provide runtime data. Hand-written completion scripts for zsh, bash, and fish are embedded as `const &str` in `cmd/completion.rs`. `wt completion install --shell <shell>` writes them to the appropriate shell completion directory.

Same behavior as current TypeScript implementation.

## TUI Changes

`crates/wt-tui/` receives `app.rs`, `ui.rs`, `terminal.rs` from existing `packages/tui/src/`. The key change: TUI calls `wt_core::worktree::*` functions instead of its own `git.rs` implementations. This eliminates the duplicated git logic that exists today.

## Distribution

### crates.io
- `wt-core` published as lib crate
- `wt-cli` published as bin crate (`cargo install wt-cli` installs `wt` binary)

### GitHub Releases
GitHub Actions cross-compiles on tag push:
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu`

Includes install script: `curl -fsSL https://raw.githubusercontent.com/listennn08/wt/main/install.sh | sh`

### npm wrapper
`packages/npm/` contains a lightweight `@listennn08/wt` package:
- `postinstall` downloads the correct prebuilt binary from GitHub Releases (no local Rust compilation)
- `bin` field points to the downloaded binary

## Migration Phases

Each phase ends with a working state that can be tested.

### Phase 1: Cargo workspace + wt-core lib
- Create workspace root `Cargo.toml`
- Move `packages/tui/src/git.rs` into `crates/wt-core/src/git.rs`, expand it
- Implement `config.rs`, `hooks.rs`, `env.rs`, `worktree.rs`

### Phase 2: Bin crate + `list` and `add` commands
- Set up clap CLI structure in `src/main.rs`
- Implement `cmd/list.rs` and `cmd/add.rs` calling wt-core
- Verifiable with `cargo run -- list` and `cargo run -- add <branch>`

### Phase 3: Remaining CLI commands
- `remove`, `switch`, `prune`, `uninstall`

### Phase 4: TUI migration
- Move `app.rs`, `ui.rs`, `terminal.rs` to `crates/wt-tui/`
- Refactor to use `wt-core` instead of internal git logic
- Wire `wt tui` command to launch TUI with tokio runtime

### Phase 5: Shell completion + output polish
- Embed completion scripts, implement `completion install`
- Verify zsh, bash, fish all work
- Finalize `output.rs` formatting

### Phase 6: Distribution + cleanup
- GitHub Actions CI workflow
- Cross-platform release builds
- npm wrapper package
- Remove `packages/core/`, `packages/mcp/`, `pnpm-workspace.yaml`
- Update CLAUDE.md and README
