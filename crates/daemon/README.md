# arugulad

The daemon: owns the terminals and blocks on a machine; clients attach over
WebSocket and the HTTP API. [AGENTS.md](../../AGENTS.md) maps its modules by
layer.

The core is in `arugula-mux` ([crates/mux](../mux/README.md)): the mux task,
panes, blocks' registry, the access model, history and the state directory.
This crate is the binary around it: the edge (the web client, `/ws`, the HTTP
API, MCP, share links, dial-out, end-to-end channels, enrolment), the
integrations (agents, browsers, editors, forges, review, invites and the
rest of the block kinds), and `src/main.rs`, which builds the mux's `Config`
from them and starts it. The mux names none of them.

Depends on `arugula-mux`, `arugula-core`, `arugula-proto`, `arugula-vt`,
`arugula-e2e` and `arugula-control-wire` (the messages it shares with
control).

Start with `src/main.rs` (startup, and where the integrations are handed to
the mux) and `src/server.rs` (the web client and `/ws`); `src/args.rs` is the
command line. The task that orders every change is `crates/mux/src/mux/mod.rs`.

Integration tests are one binary, `tests/integration/`, on
`arugula-testkit`; recorded data in `tests/fixtures/`. `tests/integration/tmux.rs`
replays iTerm2's tmux -CC session and compares every reply with tmux 3.6's.
