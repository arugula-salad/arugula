# arugula-platform

Arugula control's platform ("Control is two processes: a hub and a
platform", #1150): chat, Cloud and billing, and step 1's projects and
grants. It deploys as often as it likes without dropping a relayed terminal,
because the relay stays in the hub (`arugula-control`), which is also the
gateway for every platform path.

Today it runs inside the hub's process. Billing has moved in (M139:
`src/billing.rs`, its tables in `src/db/billing.rs`), and so has Cloud
(M137: `src/cloud/`, its tables in `src/db/cloud.rs`, its flags in
`cloud::Args`, its API types in `arugula-cloud-wire`); chat (M138) follows,
into `src/chat.rs`. M140 runs it as its own process behind the hub.

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

Depends on `arugula-control-common`, `arugula-cloud-wire` and
`arugula-e2e`, never on `arugula-control`.
