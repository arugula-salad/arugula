# arugula-platform

Arugula control's platform ("Control is two processes: a hub and a
platform", #1150): chat, hosted workspaces and billing, and step 1's projects and
grants. It deploys as often as it likes without dropping a relayed terminal,
because the relay stays in the hub (`arugula-control`), which is also the
gateway for every platform path.
It builds in the server's own Cargo workspace (`server/Cargo.toml`) with
the hub: `scripts/server-cargo test -p arugula-platform`.

Billing has moved in (M139:
`src/billing.rs`, its tables in `src/db/billing.rs`), and so have Cloud
(M137, Managoat-backed until M189 and M190 removed it: what's left in
`src/cloud/` is the `cloud.drive` check and `cloud::Args`, the master key
and beta allowlist; Cloud workspaces are hosted workspaces, `src/hosted/`)
and chat (M138:
`src/chat.rs`, its tables in `src/db/chat.rs`, its API types in
`arugula-control-wire`), and then projects and grants (`src/projects.rs`
and `src/grants.rs`, their tables in `src/db/projects.rs`), and the GitHub
App's webhook, key, access checks and tokens (M161: `src/forge/`, its flags
in `forge::Args`, the forge mirror's tables in `src/db/forge.rs`). It runs inside
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
  the hub's file; on Postgres with `ARUGULA_PLATFORM_DATABASE_URL`, M143).
- `src/db/sql.rs`: how a query runs on either SQLite or Postgres (written
  once, with SQLite's `?N`; the few that differ ask the dialect), and
  `src/db/pg.rs`: the Postgres pool behind it. `just
  test-platform-postgres` runs the suite on Postgres.
- `migrations/` and `src/db/migrate.rs`: its schema, as numbered
  migrations per SQL dialect (M142), and the runner that applies them and
  adopts a database made before them.
- `src/cache.rs`: the hub's answers the platform asks for (chat, grants,
  projects, billing, Cloud), kept and dropped as the hub's change feed
  names them: what the feed covers for as long as it's read, billing's
  meters a cache tick (a change arrives within a feed tick plus a cache
  tick, `DELAY`, and within `GRACE` while the feed is read). Every read of the
  hub's data goes through it, so a request makes no hub round trip once
  it's warm; a refusal may ask again (`*_now`), and a grant is only written
  on the hub's word. `src/hub_calls_tests.rs` holds routes to that.
- `src/projects.rs`: projects and team members, and the feed follower that
  keeps them and grants (from where it got to, in `hub_feed`).
- `src/router.rs`: its routes, which the hub mounts in process.
- `src/links.rs`: the platform page's "Linked products" (M229), which the
  hub keeps beside its issuer's clients and answers through `HubPort`.
- `src/boxes.rs`: hud boxes as clients (M237): a bound box's project's
  members and its reviews posted into object threads (kept `via` the box),
  which the hub asks for through `PlatformPort`.
- `src/planter.rs`: planter's import (M232): its backup read, and
  `arugula-platform import planter`, which links the repos planter's boxes
  were planted from to projects and reports each box
  (`docs/control-ops.md`, "Importing planter"). Its fixture backup is
  `fixtures/planter-backup`.
- `src/page.rs`: the platform page (M150's build, `web/dist-platform`),
  embedded, its CSP, and how it's served on `app.arugula.io` (M151): by
  the hub in process, by the binary for the hub's forwarded GETs out of
  process.
- `src/remote.rs`: the seam over the private network (M140): `HubPort` and
  `PlatformPort` over HTTP (`HttpHub`, `HttpPlatform`) and their servers
  (`serve_hub`, which the hub mounts on its internal listener, and
  `serve_platform`), each call signed with the shared key; and `vouched`,
  the check of the principal header every forwarded call carries.
- `src/main.rs`: the `arugula-platform` binary: its database, `HttpHub`,
  the background work (the job worker, hosted workspaces' worker, chat's
  and grants' feed followers, usage reports), and the routes behind `vouched`.
- `src/window.rs`: a window's tools (M141), in both binaries
  (`arugula-platform copy|checksums|secrets-check`, and `arugula-control
  platform …`): copy the platform's tables from one file to another,
  proven by a count and a checksum per table, and check that sealed
  secrets open with the running server's key. `src/window_drop.rs`:
  the hub's `arugula-control platform drop-copies` (#1196), which drops
  the first window's leftover copies from `control.db` once they still
  match the window's output.
- `src/agent_mcp.rs` and `src/agent_tokens.rs`: the MCP endpoint for
  agents with no machine (M230, `POST /api/mcp`), and the bearer agent
  grant tokens that open it, which only it reads. Each tool calls the
  route a machine's agents call, with the token in the machine's place.
- `src/contract.rs` (feature `contract`): the tests every `HubPort` passes.
  Control runs them against its in-process port and against `HttpHub`
  over its internal API (`hub_port.rs`).

Depends on `arugula-control-common`, `arugula-control-wire`,
`arugula-forge-github` and `arugula-e2e`, never on `arugula-control`.
