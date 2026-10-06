# illogical-control

Hosted service for accounts, devices, the directory, and the relay. It holds metadata only; terminal bytes travel end-to-end encrypted between a client and a daemon (see `docs/control-e2e.md`).

Control introduces clients and daemons and relays opaque Noise messages between them. Device keys are signed by the account, so control can refuse service but cannot read.

**Dependencies:** axum, rusqlite (SQLite), passkeys (for authentication), e2e.

**Start reading:** [`src/main.rs`](src/main.rs) for the entry point and clap arguments, then `src/routing_*.rs` for the API routes.
