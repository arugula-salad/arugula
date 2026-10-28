# arugulad-base

The daemon, as a library: it owns the terminals and blocks on a machine;
clients attach over WebSocket and the HTTP API. `arugulad`
([crates/arugulad](../arugulad/README.md)) is its binary, and calls
`arugulad_base::main` with the release it is. [AGENTS.md](../../AGENTS.md)
maps its modules by layer.

The daemon is being split into crates so an edit to one feature doesn't
recompile the rest (#232). This crate is what hasn't moved out yet. Feature
crates go on top of it, `arugulad-<area>`, and the binary wires them in.
Each new crate's name starts with `arugulad`, so `RUST_LOG=arugulad=…` still
reaches its log lines (log targets are module paths, matched by prefix). So far:
`arugulad-editor` ([crates/arugulad-editor](../arugulad-editor/README.md)),
`arugulad-drivers` (crates/arugulad-drivers, the deploy drivers), `arugulad-docs`
([crates/arugulad-docs](../arugulad-docs/README.md)), `arugulad-forge`
([crates/arugulad-forge](../arugulad-forge/README.md)), `arugulad-hosted`
([crates/arugulad-hosted](../arugulad-hosted/README.md)), `arugulad-invite`
([crates/arugulad-invite](../arugulad-invite/README.md)), `arugulad-review`
([crates/arugulad-review](../arugulad-review/README.md)), `arugulad-work`
(crates/arugulad-work, the runs engine) and `arugulad-labs`
([crates/arugulad-labs](../arugulad-labs/README.md)).
Like every library here it's version 0.0.0: what it reports as the version
comes from `arugula_core::version`, which the binary sets.

The core is in `arugula-mux` ([crates/mux](../mux/README.md)): the mux task,
panes, blocks' registry, the access model, history and the state directory.
This crate is what goes around it: the edge (the web client, `/ws`, the HTTP
API, MCP, share links, dial-out, end-to-end channels, enrolment), the
integrations (agents, browsers, editors, forges, review, invites and the
rest of the block kinds), and `src/lib.rs`'s `main`, which builds the mux's
`Config` from them and starts it. The mux names none of them.

Depends on `arugula-mux`, `arugula-core`, `arugula-proto`, `arugula-vt`,
`arugula-e2e` and `arugula-control-wire` (the messages it shares with
control).

Start with `src/lib.rs` (startup, and where the integrations are handed to
the mux) and `src/server.rs` (the web client and `/ws`); `src/args.rs` is the
command line. The task that orders every change is `crates/mux/src/mux/mod.rs`.

Its unit tests are beside the code. The integration tests are the binary's
(`crates/arugulad/tests/integration/`); their recorded data, and the fake
agents they run, stay here in `tests/` (`tests/fixtures/`, `tests/fake_*.py`),
where other crates and the web client's specs read them too.
