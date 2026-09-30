# wt

A git worktree manager — one Rust binary with a CLI and an interactive TUI.

Worktrees let you check out several branches at once, each in its own directory.
The friction is everything around them: picking a path, copying `.env` files over,
reinstalling dependencies, and remembering which directory holds which branch.
`wt` handles that part.

```
$ wt add feature/login
$ wt list
BRANCH         PATH                          HEAD      FLAGS
main           ~/dev/myapp                   8770fef4  base
feature/login  ~/dev/myapp_feature-login     8770fef4
```

## Install

```bash
# Homebrew (macOS / Linux)
brew tap listennn08/wt https://github.com/listennn08/wt
brew install wt

# Cargo
cargo install wt-cli

# npm
npm install -g @listennn08/wt

# Shell script
curl -fsSL https://raw.githubusercontent.com/listennn08/wt/main/install.sh | sh
```

`wt` shells out to `git`, so a `git` binary must be on your `PATH`.

Then install shell completion (zsh, bash, or fish — auto-detected):

```bash
wt completion install
```

## Commands

### `wt add <branch>`

Creates a worktree at `../<repo>_<branch>` (slashes in the branch name become
dashes). Picks the right `git worktree add` variant for you:

| Situation | What happens |
|---|---|
| Branch exists locally | Checks it out |
| Branch exists on the remote only | Creates a local branch tracking it |
| Branch doesn't exist | Creates it from the current branch |

The remote is only consulted when the branch is missing locally, so the common
case involves no network round-trip.

```bash
wt add feature/login              # ../myapp_feature-login
wt add hotfix -d ~/tmp/hotfix     # explicit directory
wt add spike -b main              # branch off main instead of HEAD
wt add feature/login -n           # force a new branch, ignore origin/feature/login
wt add existing -f                # allow a directory that already exists
```

After creating the worktree, `wt` copies `.env` and `.env.local` from the base
worktree if the destination doesn't already have them.

### `wt list`

```bash
wt list           # aligned table
wt list --json    # machine-readable
wt list --raw     # plain `git worktree list` output
```

Flags shown per row: `base` (the main worktree), `locked`, `prunable`.

### `wt switch <target>` (alias `sw`)

Opens a shell in the worktree's directory. `<target>` may be a branch name or a
path — `wt` figures out which.

```bash
wt switch feature/login
wt switch --print feature/login   # just print the path (useful in scripts)
wt switch -b main                 # resolve as a branch name only
wt switch -p ../myapp_hotfix      # resolve as a path only
```

Because a child process can't change its parent's directory, this starts a
subshell. `exit` returns you to where you were. To `cd` instead, use `--print`:

```bash
cd "$(wt switch --print feature/login)"
```

### `wt remove <target>` (alias `rm`)

```bash
wt remove feature/login       # by branch or path
wt rm -b feature/login        # by branch name only
wt rm -p ../myapp_hotfix      # by path only
wt rm feature/login -f        # force, even with uncommitted changes
```

### `wt prune`

```bash
wt prune --dry-run    # show what would go
wt prune --verbose
wt prune --expire 2.weeks.ago
```

### `wt tui`

An interactive worktree browser with an embedded terminal per worktree. Each
worktree gets its own shell, started in its directory and kept alive as you move
around.

**Worktree list**

| Key | Action |
|---|---|
| `↑` `↓` / scroll | Move selection |
| `Enter` / `Tab` | Focus the terminal pane |
| `a` | Add a worktree |
| `r` | Remove the selected worktree |
| `x` | Prune |
| `g` | Refresh |
| `R` | Restart the shell |
| `q` | Quit |

**Terminal pane**

| Key | Action |
|---|---|
| `Ctrl+T` | Back to the list |
| Scroll | Scroll back through history |

Every other key goes to the shell — `Esc` for vim, `Ctrl+R` for history
search, `Alt` combinations for word movement. When the shell exits, focus
returns to the list; `R` there starts a new one.

**Add-worktree prompt** (`a`)

| Key | Action |
|---|---|
| `Tab` | Complete to the longest prefix shared by the matching branches |
| `Ctrl+W` | Delete one path segment |
| `Ctrl+U` | Clear the field |
| `Enter` / `Esc` | Create / cancel |

Existing branches matching what you've typed are listed under the field — local
branches plus remote ones with their `<remote>/` prefix stripped, which is the
form `wt add` accepts. Pasting works in the prompt and in the terminal pane.

Confirmation prompts take `Enter`/`y` to accept, `Esc`/`n` to cancel. The list
refreshes on its own when git state changes underneath it.

## Configuration

Optional. `wt` looks for the first of these that exists:

1. `<repo>/.wt.toml`
2. `<repo>/wt.toml`
3. `<repo>/.config/wt/config.toml`
4. `$XDG_CONFIG_HOME/wt/config.toml`, or `~/.config/wt/config.toml`

### Hooks

Run commands around worktree creation — install dependencies, link caches,
whatever the project needs:

```toml
# .wt.toml
[[hooks.add.post_create]]
program = "pnpm"
args = ["install"]
cwd = "${worktree}"

[[hooks.add.post_create]]
program = "ln"
args = ["-s", "${base}/node_modules/.cache", "${worktree}/node_modules/.cache"]

# Skip the built-in .env copying
[hooks.add]
disable_default_post_create = true
```

`pre_create` hooks run before the worktree exists; `post_create` after.

`${base}` and `${worktree}` are substituted anywhere in `args` and `cwd`. The
same values also arrive as environment variables:

| Variable | Value |
|---|---|
| `WT_BASE` | Path of the base worktree |
| `WT_WORKTREE` | Path of the new worktree |
| `WT_BRANCH` | Branch name |

`cwd` defaults to the base worktree. A failing hook aborts the operation and
reports the command, working directory, and exit code.

## Uninstall

```bash
wt uninstall            # removes completions for the detected shell
wt uninstall --shell all
cargo uninstall wt-cli  # or: brew uninstall wt
```

## Development

```
crates/wt-core   git operations, config, hooks, env copying
crates/wt-tui    ratatui TUI — app state, rendering, PTY sessions
src/             clap CLI, one module per command in src/cmd/
packages/npm     npm wrapper that downloads prebuilt binaries
```

```bash
cargo build              # debug
cargo build --release
cargo run -- list        # run in dev
cargo test               # unit tests
cargo check --workspace
```

Two design notes worth knowing before you change things:

- **No async runtime.** The TUI drives its PTYs from reader threads. Nothing here
  needs to be `async`.
- **Git via subprocess.** `wt-core` shells out to `git` — notably
  `git worktree list --porcelain` — rather than linking libgit2. Fewer
  dependencies, and no second implementation of git's own semantics to keep in
  sync.

## License

ISC
