# Arugula

The `arugula` command: drive arugulad from a shell or a script, over the
daemon's HTTP API on its Unix socket (or another daemon's URL, with
`--host`). `--json` prints the API's answers as they are.

Depends on `arugula-core`, `arugula-proto`, `arugula-control-wire` and
`arugula-e2e`.

Start with `src/main.rs` (the command tree and dispatch); each subcommand is
in `src/cmd/<name>.rs`, shared helpers in `src/util.rs`. `src/ask.rs` and
`src/hook.rs` are Claude Code's hooks, `src/mcp.rs` the stdio bridge to the
daemon's MCP server. `src/attach.rs` is `arugula attach`, one pane in this
terminal. The CLI doesn't link `arugula-vt` (no libghostty, no Zig): the TUI
and tmux -CC that needed it were removed (DECISIONS.md).
