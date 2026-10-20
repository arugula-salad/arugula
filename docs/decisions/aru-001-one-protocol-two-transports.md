---
schema: 1
id: "aru-001"
title: "One protocol, two transports"
state: "decided"
area: "Protocol and transports"
source:
  via: "cli"
question: "One protocol, two transports"
options:
  - id: "a"
    label: "One protocol, two transports"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Protocol"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#protocol-one-protocol-two-transports"
  - title: "M4"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/proto/src/lib.rs"
  - "path:crates/daemon/src/server.rs"
  - "path:crates/daemon/src/pipe.rs"
---

# One protocol, two transports

The web client, the CLI and the API speak one protocol. On a
WebSocket it is JSON text frames for control messages and binary frames for
terminal bytes; on the Unix socket (a named pipe on Windows) the same messages
travel length-prefixed. A transport never shapes the protocol or the core, so
a new way to reach a daemon is an adapter.
Where: `crates/proto/src/lib.rs` (`ClientMsg`, `ServerMsg`, `Frame`), `crates/daemon/src/server.rs` (`/ws`), `crates/daemon/src/pipe.rs`.
From: [Protocol](docs/plan-archive.md#protocol-one-protocol-two-transports), [M4](docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox).
