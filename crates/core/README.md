# illogical-core

Pure layout state for sessions, tabs, splits, and the cell grid. Sessions hold tabs; tabs hold a split tree of panes. Changes come through `Intent`s, and layout is property-tested.

The mux task in the daemon applies intents and uses this crate's types to compute the layout the web client draws.

**Dependencies:** serde (and ts-rs as an optional feature for TypeScript generation).

**Start reading:** [`src/lib.rs`](src/lib.rs) for the public types, then [`src/mux.rs`](src/mux.rs) for the intent handlers, and [`src/layout.rs`](src/layout.rs) for cell grid computation.
