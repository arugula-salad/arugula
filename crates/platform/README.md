# arugula-platform

Arugula control's platform ("Control is two processes: a hub and a
platform", #1150): chat, Cloud and billing, and step 1's projects and
grants. It deploys as often as it likes without dropping a relayed terminal,
because the relay stays in the hub (`arugula-control`), which is also the
gateway for every platform path.

Today it runs inside the hub's process and holds nothing yet. Cloud (M137),
chat (M138) and billing (M139) move in, each into its own module
(`src/cloud/`, `src/chat.rs`, `src/billing.rs`); M140 runs it as its own
process behind the hub.

Start with `src/lib.rs`, then:
- `src/hub.rs`: `HubPort`, everything the platform asks the hub, and the
  change feed's kinds. The platform never reads a hub table.
- `src/port.rs`: `PlatformPort`, everything the hub asks the platform.
- `src/auth.rs`: who's asking, as the hub found out, and the extractors
  that answer as control's `Session` and `DaemonAuth` do.
- `src/db.rs`: its own database handle (in process, a second connection to
  the hub's file).
- `src/router.rs`: its routes, which the hub mounts.
- `src/contract.rs` (feature `contract`): the tests every `HubPort` passes.
  Control runs them against its in-process port (`hub_port.rs`).

Depends on `arugula-control-common` and `arugula-e2e`, never on
`arugula-control`.
