# arugula-control

Arugula control: accounts, devices, the directory and the relay, for
people who don't run a tailnet and for teams, and the platform plane: what
has to exist while every machine is asleep (chat, Cloud workspaces, Shared
Docs, billing). Terminal bytes travel end to end between a device and a
daemon ([docs/control-e2e.md](../../docs/control-e2e.md), [docs.arugula.io/control](https://docs.arugula.io/control/)). Running
and operating it: [docs/control-ops.md](../../docs/control-ops.md).

Control is two processes (#1150): this crate is the hub, and
`crates/platform` the platform, which runs in this process
(`--platform in-process`, the default) or as its own behind this hub's
gateway (M140, `src/gateway.rs`: `App::platform` is a `PlatformAt`). The
platform reaches the hub only through `HubPort`, whose in-process side is
`src/hub_port.rs` (with `who` and `gate`, which tell the platform's routes
who's asking), served over HTTP on the hub's internal listener
(`src/internal.rs`), and follows the hub through its outbox
(`src/db/changes.rs`). `src/maintenance.rs` is the switch that answers
every platform route with 503 for a window (M141), and `src/db/platform.rs`
what the hub keeps about the platform. `src/gateway_wire.rs` tests the
gateway over real sockets. Build an `App` with `App::start`, which
attaches the platform's `HubPort` to it.

Every table control's database has is listed in [tables.toml](tables.toml),
with its plane, which process owns it, what it holds and who else sees it. A test fails when a
table isn't there: a new table gets a row, and `just control-tables`
rewrites "What control stores" in docs/control-e2e.md from it.

Every route control registers is listed in [routes.toml](routes.toml), with
the process that answers it once control is split (hub or platform, #1150),
what it checks and who calls it. A test fails when a route isn't there: a
new route gets a row.

Depends on `arugula-e2e`, `arugula-control-wire` (the messages it shares
with daemons and the CLI), `arugula-control-common` (what it shares with the
platform) and `arugula-platform`.

Start with `src/main.rs`, then `src/api.rs` (devices, approvals, daemons
joining, the directory) and `src/relay.rs` (the relay). `src/blocks.rs`
serves blocks' own origins under `--block-domain` (M50), with their
scripts from `web/src/shell/block/`. `src/issuer.rs` is the hub as an issuer (M228): OIDC-shaped
authorize, token and userinfo endpoints, the JWKS, and hud's assertions,
for products registered in `issuer_clients`; the token formats and how to
verify them are `arugula_control_common::issuer`, and
`src/issuer_wire.rs` tests the flow, rotation included.
`src/platform_host.rs` serves the
platform page's own host, `--platform-host` (M151: `app.arugula.io`), with
only the platform's routes and its page (`crates/platform/src/page.rs`). Chat (M122: a team's channels, DMs and
threads about panes, which control holds in plain text) is the platform's
since M138 (`crates/platform/src/chat.rs`); `src/chat_wire.rs` tests it
through this hub's gate and `hub_port::InProcess`, and `src/chat_bench.rs`
times `GET /api/chat` by hand. `src/hub_latency_bench.rs` times platform
routes out of process with a delay on every hub call, as between regions.
The platform plane's nouns (#1122), projects, `team_members` and grants,
are the platform's since step 2 (`crates/platform/src/db/projects.rs`,
`projects.rs` and `grants.rs`): no hub query names them
(`db::tables::the_hub_names_no_platform_table`). `src/grants_wire.rs` tests
grants through this hub's gate.
