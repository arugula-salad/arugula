# arugula-platform-api

The hub's side of the seam between Arugula control's hub and its platform
(#314). The hub (`arugula-control-base`, crates/control) builds against this
crate and never against the platform's base (`arugula-platform-base`,
crates/platform), so a platform edit doesn't rebuild the hub's 38k lines.

- `src/hub.rs`: `HubPort`, everything the platform asks the hub.
- `src/port.rs`: `PlatformPort`, everything the hub asks the platform (the
  platform answers it in `crates/platform/src/port.rs`). Both ports' types
  are `arugula_control_wire::internal`'s.
- `src/remote.rs`: both ports over the private network (M140), each call
  signed with the shared key, and `vouched`.
- `src/auth.rs`: who's asking, as the hub found out.
- `src/page.rs`: the platform page (`web/dist-platform`, embedded; this
  crate's `build.rs` and `debug-embed`).
- `src/in_process.rs`: the platform in the hub's process. The
  `arugula-control` binary hands the hub `Platforms` (the platform's flags
  and how to make it, `arugula_platform::in_process::HubPlatforms`); the hub
  holds an `InProcess`.
- `src/planter_backup.rs`: planter's backup as both imports read it.
- `src/lib.rs`: `Config`, and the few names and checks both sides use.

Everything here is the contract `scripts/ci-hub-changed` always compares.
