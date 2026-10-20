# arugula-control

Arugula control: accounts, devices, the directory and the relay, for
people who don't run a tailnet and for teams, and the platform plane: what
has to exist while every machine is asleep (chat, Cloud workspaces, Shared
Docs, billing). Terminal bytes travel end to end between a device and a
daemon ([docs/control-e2e.md](../../docs/control-e2e.md), [docs.arugula.io/control](https://docs.arugula.io/control/)). Running
and operating it: [docs/control-ops.md](../../docs/control-ops.md).

Every table control's database has is listed in [tables.toml](tables.toml),
with its plane, what it holds and who else sees it. A test fails when a
table isn't there: a new table gets a row, and `just control-tables`
rewrites "What control stores" in docs/control-e2e.md from it.

Depends on `arugula-e2e` and `arugula-control-wire` (the messages it
shares with daemons and the CLI).

Start with `src/main.rs`, then `src/api.rs` (devices, approvals, daemons
joining, the directory) and `src/relay.rs` (the relay). `src/blocks.rs`
serves blocks' own origins under `--block-domain` (M50), with their
scripts from `web/src/block/`. `src/chat.rs` (with `src/db/chat.rs`) is
chat (M122): a team's channels, DMs and threads about panes, which control
holds in plain text.
