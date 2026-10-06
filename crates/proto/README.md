# illogical-proto

Wire protocol for client–daemon and daemon–daemon communication: `ClientMsg` and `ServerMsg` for JSON control messages, and `Frame` for binary terminal bytes with a fixed header and stream offset.

The web client's TypeScript copy (`web/src/proto.gen.ts`) is generated from the Rust types with ts-rs. Run `just proto-ts` after changes (CI fails if stale). The HTTP API types are still written by hand in `proto.rs`.

**Dependencies:** serde, serde_json, and ts-rs (as an optional feature).

**Start reading:** [`src/lib.rs`](src/lib.rs) for the wire types and the protocol overview at the top.
