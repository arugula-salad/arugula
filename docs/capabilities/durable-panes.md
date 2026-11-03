---
id: "durable-panes"
title: "Durable panes"
verified_through: "665cdb10ed705245af5afaf697068c829f8504b6"
entry: ["crates/daemon/src/server.rs", "crates/cli/src/attach.rs", "web/src/shell/client.ts"]
paths: ["crates/mux/src/pane.rs", "crates/mux/src/store.rs", "crates/mux/src/shim.rs", "crates/mux/src/holder.rs", "crates/mux/src/sys.rs", "crates/mux/src/host.rs", "crates/mux/src/conpty.rs", "crates/mux/src/resume.rs", "crates/vt/src/ghostty/wire.rs"]
shared: ["crates/proto/src/lib.rs", "crates/mux/src/mux/mod.rs", "crates/mux/src/mux/clients.rs", "crates/mux/src/mux/config.rs", "crates/daemon/src/lib.rs", "crates/daemon/src/install.rs"]
tests: ["crates/arugulad/tests/integration/attach.rs", "crates/arugulad/tests/integration/upgrade.rs", "crates/arugulad/tests/integration/reboot.rs", "crates/arugulad/tests/integration/resume.rs", "crates/arugulad/tests/integration/windows.rs", "web/e2e/reattach.spec.ts"]
decisions: ["aru-002", "aru-005", "aru-006", "aru-007", "aru-009", "aru-010", "aru-011", "aru-012", "aru-013"]
---
# Durable panes

## What a person can do

A terminal pane keeps running on its machine when every client has gone. A client that can reach the daemon attaches and gets the screen and scrollback, and one that dropped resumes from where it stopped. This needs no flag or sign-in on the machine itself, but whether a pane's program survives a daemon restart depends on how the daemon was started (see below).

## Where it runs

One `arugulad` per machine owns the panes. Each program runs under `arugulad _shim` (`shim::run`), which forks it on the PTY and appends its pid and how it ended to the pane's `process` record. On Windows, `arugulad _host` (`host.rs`) does this and owns the pseudoconsole. A pane's thread (`spawn_pane` in `pane.rs`) owns its terminal engine and writes output to the log.

The state is files in the machine's state dir (`store.rs`), private to the user: `layout.json`, and for each pane `blocks/<id>/` with `seg-<offset>.log`, `index` and `checkpoint`. Retention is 256 MiB of log per pane (`RETAIN_BYTES`). A closed pane's directory is kept 7 days (`CLOSED_RETENTION_MS`).

Keeping a terminal open across a restart is decided by `Launcher::detect`. Under a systemd service, each pane gets its own scope and its PTY master waits in the FD store as `pane-<id>` (`sys::store_fd`, read back by `take_listen_fds`). Without systemd, `--keep-panes` or `ARUGULA_KEEP_PANES` makes the shim hold the master and lend it over a socket (`holder::borrow`, `holder::collect`). It hangs up after `holder::GRACE` (60 s) with no daemon. The macOS launch agent (`install.rs`), the ssh installer (`crates/cli/src/ssh.rs`) and hosted boxes set this. With neither, the pane's `kept` is false and it ends with the daemon, and the next daemon restores it by policy.

At startup `Daemon::restore` (`crates/mux/src/mux/mod.rs`) loads the layout. A pane with a kept terminal and a live record (`shim::alive`) becomes `Start::Adopt`; any other goes through `config.restore`, which applies the pane's `Policy` or resumes its agent conversation (`resume::plan`). `restore_engine` rebuilds the screen from the checkpoint and the log after it, at most 8 MiB of log (`RESTORE_REPLAY_BYTES`), and `restored_banner` marks the join.

Clients arrive at `/ws` (`server::ws`), the Unix socket's `local_ws` (`arugula attach`, as the owner), or an end-to-end channel (`crates/daemon/src/e2e.rs`). All become a mux client through `Cmd::Connect`. `ClientMsg::Attach` reaches `Daemon::message` (`crates/mux/src/mux/clients.rs`), which skips any pane the caller can't read (`readable`) and sends it to the pane's thread (`State::attach` in `pane.rs`). An offset within the last 1 MiB (`MAX_REPLAY_BYTES`) gets an `Output` replay; anything else gets a `Snapshot`, or `SnapshotZstd` for a client that asks. The web client asks for 10,000 rows with zstd and acks; `arugula attach` asks for neither. A client that acks is held to 512 KiB unacked (`ACK_WINDOW`). The size of a pane is its tab's, set as aru-007 says.

Who may attach is `readable` in `crates/mux/src/mux/who_may.rs`: the owner, or someone with a role on the session if the pane isn't private. A "from now" share gets a floor (`Acl::floor`): no replay below it, and a snapshot with no history.

## Compatibility

- `layout.json` carries `version` (`LAYOUT_VERSION`, 1). A file that doesn't parse or has another version is moved aside as `layout.json.unreadable-<ms>` and the daemon starts with no restored panes. Fields added to `Saved` and `PaneMeta` since use `serde(default)`.
- A checkpoint that doesn't decode, or is too far behind, is ignored and the log tail replayed (aru-009, `restore_engine`).
- Frames are `[u8 kind][u32 pane][u64 offset][payload]`, big-endian (aru-002). An unknown kind is a decode error. `Attach`'s optional fields default, and `kitty_keys` stays unused so older clients and daemons agree.
- Each pane's `epoch` is new with each daemon start. The web client drops its offset when the epoch changes and asks for a snapshot (`applyState` in `client.ts`).
- A daemon finds a shim's socket from before the rename to Arugula (`holder::sockets_for`).
- `PROTOCOL` in `crates/proto/src/lib.rs` (1) is the desktop app's range check against a daemon, bumped only for a change that breaks an older app.
- A reboot ends programs. Stopping the daemon ends them too, within seconds under systemd and after the grace period under a shim (`upgrade.rs`). Layout and scrollback come back, and the policy decides what runs (`attach.rs`, `reboot.rs`).

## Not known

- What we promise about durability, and on which installs. `README.md` says restarting or updating the daemon keeps programs running, with no condition, but a daemon started by hand on Linux with no systemd and no `--keep-panes` ends them.
- What "Scrollback always comes back" (aru-012) means in size. The limits I found are 256 MiB of log, 8 MiB replayed on restore, 16 MiB in the engine (`SCROLLBACK_BYTES`) and 10,000 rows on the web client.
- Whether a real older binary's panes get adopted. `upgrade.rs` restarts the same build, and I found no test across versions.
- What a laptop sleeping does to the shim's 60-second grace. I found no test of it.
- How the desktop app attaches. I traced the web client and `arugula attach`, not `crates/desktop`.
- Which CI jobs run the system tests. `upgrade.rs` skips its systemd test without a systemd user manager, and `reboot.rs` needs Docker.
