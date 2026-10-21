# arugula-platform

Arugula control's platform ("Control is two processes: a hub and a
platform", #1150): chat, Cloud and billing, and step 1's projects and
grants. It deploys as often as it likes without dropping a relayed terminal,
because the relay stays in the hub (`arugula-control`), which is also the
gateway for every platform path.

Billing has moved in (M139:
`src/billing.rs`, its tables in `src/db/billing.rs`), and so have Cloud
(M137: `src/cloud/`, its tables in `src/db/cloud.rs`, its flags in
`cloud::Args`, its API types in `arugula-cloud-wire`) and chat (M138:
`src/chat.rs`, its tables in `src/db/chat.rs`, its API types in
`arugula-control-wire`), and then projects and grants (`src/projects.rs`
and `src/grants.rs`, their tables in `src/db/projects.rs`). It runs inside
the hub's process (`--platform in-process`, the default) or as its own,
the `arugula-platform` binary (`src/main.rs`, M140), behind the hub's
gateway ([docs/control-ops.md](../../docs/control-ops.md#the-platform)).

Start with `src/lib.rs`, then:
- `src/hub.rs`: `HubPort`, everything the platform asks the hub, and the
  change feed's kinds. The platform never reads a hub table.
- `src/port.rs`: `PlatformPort`, everything the hub asks the platform.
- `src/auth.rs`: who's asking, as the hub found out, and the extractors
  that answer as control's `Session` and `DaemonAuth` do.
- `src/db.rs`: its own database handle (in process, a second connection to
  the hub's file).
- `migrations/` and `src/db/migrate.rs`: its schema, as numbered
  migrations per SQL dialect (M142), and the runner that applies them and
  adopts a database made before them.
- `src/cache.rs`: the hub's answers chat asks for, kept and dropped as the
  hub's change feed names them (a feed tick plus a cache tick, `DELAY`).
- `src/projects.rs`: projects and team members, and the feed follower that
  keeps them and grants (from where it got to, in `hub_feed`).
- `src/router.rs`: its routes, which the hub mounts in process.
- `src/remote.rs`: the seam over the private network (M140): `HubPort` and
  `PlatformPort` over HTTP (`HttpHub`, `HttpPlatform`) and their servers
  (`serve_hub`, which the hub mounts on its internal listener, and
  `serve_platform`), each call signed with the shared key; and `vouched`,
  the check of the principal header every forwarded call carries.
- `src/main.rs`: the `arugula-platform` binary: its database, `HttpHub`,
  the background work (the cloud worker, chat's and grants' feed
  followers, usage reports), and the routes behind `vouched`.
- `src/window.rs`: a window's tools (M141), in both binaries
  (`arugula-platform copy|checksums|secrets-check`, and `arugula-control
  platform …`): copy the platform's tables from one file to another,
  proven by a count and a checksum per table, and check that cloud secrets
  open with the running server's key.
- `src/contract.rs` (feature `contract`): the tests every `HubPort` passes.
  Control runs them against its in-process port and against `HttpHub`
  over its internal API (`hub_port.rs`).

Depends on `arugula-control-common`, `arugula-cloud-wire`,
`arugula-control-wire` and `arugula-e2e`, never on `arugula-control`.
