# arugula-control

Arugula control: accounts, devices, the directory and the relay, for
people who don't run a tailnet and for teams. It holds metadata only:
terminal bytes travel end to end between a device and a daemon
([docs/control-e2e.md](../../docs/control-e2e.md), [docs.arugula.io/control](https://docs.arugula.io/control/)). Running
and operating it: [docs/control-ops.md](../../docs/control-ops.md).

Depends on `arugula-e2e` and `arugula-control-wire` (the messages it
shares with daemons and the CLI).

Start with `src/main.rs`, then `src/api.rs` (devices, approvals, daemons
joining, the directory) and `src/relay.rs` (the relay). `src/blocks.rs`
serves blocks' own origins under `--block-domain` (M50), with their
scripts from `web/src/block/`. `src/chat.rs` (with `src/db/chat.rs`) is
chat (M122): a team's channels, DMs and threads about panes, which control
holds in plain text, the one exception to "metadata only".
`src/db/projects.rs` holds the platform plane's nouns (#1122): projects,
which Cloud workspaces, team channels and cloud secrets belong to,
`team_members` (a projection of each signed roster, never read for machine
access) and grants.
