# illogical-e2e

End-to-end encryption between clients and daemons over the control relay. Implements the Noise IK protocol (with snow crate) for session establishment and key derivation.

Each device holds two key pairs: X25519 for Noise and Ed25519 for signing. Keys are persisted to disk and used to prove device identity to the control service.

**Dependencies:** snow (Noise protocol), ed25519-dalek, serde.

**Start reading:** [`src/lib.rs`](src/lib.rs) for the overview and `docs/control-e2e.md` in the repo root for the full design.
