# illogical

Command-line interface to illogicald. Talks to the daemon's HTTP API over its Unix socket, or to another daemon's URL with `--host`. Supports `--json` for machine-readable output.

Includes three frontends:
- **CLI:** subcommands like `illogical attach`, `illogical ssh`, and `illogical ask` (the Claude Code integration point).
- **TUI:** a terminal UI that mirrors the daemon's state (no new features after launch).
- **tmux -CC:** a tmux control-mode adapter that lets tmux clients attach (recorded fixtures in `crates/daemon/tests/integration/tmux.rs`).

**Dependencies:** proto, daemon (test only), tokio, clap.

**Start reading:** [`src/main.rs`](src/main.rs) for the command tree, then see `src/cmd/` for each command's implementation. The tmux adapter is in `src/tmux/`.
