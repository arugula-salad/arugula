# illogical-testkit

Harness for illogicald's integration tests: a dev daemon with its own state directory and socket, helpers for making requests to it, and cleanup when the harness is dropped.

Used only by tests. See `docs/testing.md` for how to use it.

**Dependencies:** proto, daemon, tokio, anyhow.

**Start reading:** [`src/lib.rs`](src/lib.rs) for the `Testkit` struct and its methods.
