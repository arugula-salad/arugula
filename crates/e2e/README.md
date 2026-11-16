# arugula-e2e

End-to-end encryption between client devices and daemons, for the control
track. It was built so control could relay but not read; since aru-141 the
hub's machine tokens decide who reaches a machine instead, control can read
what it relays, and the device-key parts of this crate are to be deleted
(#1007). The design is in [docs/control-e2e.md](../../docs/control-e2e.md).

Depends on no other workspace crate.

Start with `src/channel.rs` (Noise IK between a device and a daemon), then
`src/keys.rs` (a device's X25519 and Ed25519 keys). The browser's side is in
`web/src/shell/e2e`; `just e2e-interop` checks the two against each other.

The logic the hosted server shares with the daemon (certificates, teams,
deploys, drivers' plans, runs, hosted machines) is pinned by example files in
[contract/examples/logic](../../contract/examples/logic/README.md), which
`tests/logic` writes and checks.
