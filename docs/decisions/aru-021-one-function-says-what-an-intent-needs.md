---
schema: 1
id: "aru-021"
title: "One function says what an intent needs"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
question: "One function says what an intent needs"
options:
  - id: "a"
    label: "One function says what an intent needs"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M12"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m12-principals-and-roles"
  - title: "Multiplayer track"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#multiplayer-track-m12m15-added-2026-10-01"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/core/src/access.rs"
  - "path:crates/mux/src/acl.rs"
  - "path:crates/daemon/src/authz.rs"
  - "path:crates/mux/src/mux/who_may.rs"
---

# One function says what an intent needs

Every request has an author and every session an access list. Roles per
session are `viewer`, `editor` and `owner`; the daemon's owner owns every
session. `illogical_core::access::need` is the one decision function, and the
daemon enforces it on every path: the WebSocket (in the mux, per message), the
HTTP API (`authz.rs`), the Unix socket and the channel.
The unit of sharing is the session; tabs, blocks and machines inherit it.
Grants are data (`acl.json`) with an audit log (`audit.jsonl`).
Where: `crates/core/src/access.rs`, `crates/mux/src/acl.rs`, `crates/daemon/src/authz.rs`, `crates/mux/src/mux/who_may.rs`.
From: [M12](docs/plan-archive.md#m12-principals-and-roles), [Multiplayer track](docs/plan-archive.md#multiplayer-track-m12m15-added-2026-10-01).
