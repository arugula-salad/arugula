---
schema: 1
id: "aru-017"
title: "The mux names no integration"
state: "decided"
area: "The mux and panes"
source:
  via: "cli"
question: "The mux names no integration"
options:
  - id: "a"
    label: "The mux names no integration"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "docs/mux-crate.md"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/mux-crate.md"
  - title: "#459"
    url: "https://github.com/arugula-salad/arugula/issues/459"
  - title: "#387"
    url: "https://github.com/arugula-salad/arugula/issues/387"
  - title: "#460"
    url: "https://github.com/arugula-salad/arugula/issues/460"
  - title: "#701"
    url: "https://github.com/arugula-salad/arugula/issues/701"
  - title: "#705"
    url: "https://github.com/arugula-salad/arugula/issues/705"
decided_by: "jhgaylor"
decided_on: "2026-10-09"
reviews: []
constrains:
  - "path:crates/mux/src/mux/outside.rs"
  - "path:crates/mux/src/block.rs"
  - "path:crates/mux/src/mux/config.rs"
  - "path:crates/daemon/src/main.rs"
  - "path:crates/mux/README.md"
---

# The mux names no integration

`arugula-mux` holds the core (the mux task, panes, blocks and their registry,
the access model, history, the state directory) and `arugulad` the binary
around it: the edge, MCP, and every integration. The line runs one way: the
daemon names the mux, never the other. Nothing in the mux names an HTTP
server or client, MCP, SSH, the embedded web client or control. Three
mechanisms carry what crosses it: registration (the daemon adds a `BlockKind`
per block type to `BlockKinds` before the mux starts), traits (what the mux
calls out to is a trait in `Config`, in `mux/outside.rs`, and `Config::at_exit`
for the daemon's own cleanup), and features (`arugula-mux/labs`, which the
daemon's `labs` forwards). `just mux-guard`, run by `just check` and
`just check-core`, fails if `axum`, `hyper`, `reqwest`, `rmcp`, `rust-embed` or
`russh` is in `cargo tree -p arugula-mux`, with or without `labs`.
Why: a core that can't reach an integration can't depend on one by accident, so
it stays testable and movable without them, and the order of changes stays
decided in one task.
Where: `crates/mux/src/mux/outside.rs`, `crates/mux/src/block.rs` (`BlockKinds`), `crates/mux/src/mux/config.rs` (`Config`), `crates/daemon/src/main.rs` (the wiring), `justfile` (`mux-guard`), `crates/mux/README.md`.
From: [docs/mux-crate.md](docs/mux-crate.md), #459 (the design, #387 item 4), moved in #460 and #701–#705.
