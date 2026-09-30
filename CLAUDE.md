# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`wt` is a git worktree manager — a single Rust binary with CLI commands and an interactive TUI.

## Structure

- **crates/wt-core** — lib crate: git operations (git2), TOML config, hooks, env copying, worktree orchestration
- **crates/wt-tui** — lib crate: Ratatui TUI (app state, rendering, PTY sessions)
- **src/** — bin crate: clap CLI routing, per-command modules in `src/cmd/`
- **packages/npm/** — lightweight npm wrapper that downloads prebuilt binaries from GitHub Releases

## Build Commands

```bash
cargo build                    # debug build
cargo build --release          # release build
cargo run -- <subcommand>      # run in dev
cargo check                    # type check all crates
```

```bash
cargo test                     # unit tests (parsing, formatting, hook expansion)
```

## Architecture Notes

- **Single binary**: CLI commands and TUI share `wt-core` — no duplicated git logic.
- **Fully synchronous**: no async runtime anywhere. The TUI drives its PTYs from reader threads.
- **Git via subprocess**: `wt-core` shells out to `git` (notably `worktree list --porcelain`) rather than linking libgit2.
- **Hook system**: TOML-based lifecycle hooks (`pre_create`, `post_create`) in `.wt.toml`. Variables: `${base}`, `${worktree}`, env vars `WT_BASE`, `WT_WORKTREE`, `WT_BRANCH`.
- **Config resolution**: `.wt.toml` → `wt.toml` → `.config/wt/config.toml` → `~/.config/wt/config.toml` → `$XDG_CONFIG_HOME/wt/config.toml`.
- **Shell completion**: hand-written scripts (zsh/bash/fish) with hidden subcommands (`__complete-branches`, `__complete-worktrees`, `__complete-actions`) for dynamic branch/worktree lookup.
- **Distribution**: Homebrew tap (`Formula/wt.rb`, regenerated on release by `scripts/update-formula.sh`), crates.io (`cargo install wt-cli`), GitHub Releases (prebuilt binaries), npm wrapper (`@listennn08/wt`).
- **Homebrew formula is generated, not hand-edited**: the release workflow computes checksums from the published tarballs and commits `Formula/wt.rb` back to `main`.
