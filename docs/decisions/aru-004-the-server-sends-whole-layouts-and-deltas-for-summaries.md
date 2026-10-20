---
schema: 1
id: "aru-004"
title: "The server sends whole layouts, and deltas for summaries"
state: "decided"
area: "Protocol and transports"
source:
  via: "cli"
question: "The server sends whole layouts, and deltas for summaries"
options:
  - id: "a"
    label: "The server sends whole layouts, and deltas for summaries"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M23"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m23-pane-summaries"
  - title: "spikes list"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#s16-swarm-spike-summary-cost-fleet-connections-canvas"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/proto/src/lib.rs"
---

# The server sends whole layouts, and deltas for summaries

`State` carries the full tree and a `rev` on every layout change (trees are
small). Pane summaries and attention, which change often and across many
hosts, go as field-level `Delta`s.
Where: `crates/proto/src/lib.rs` (`ServerMsg::State`, `Delta`).
From: [M23](docs/plan-archive.md#m23-pane-summaries), S16 (in the [spikes list](docs/plan-archive.md#s16-swarm-spike-summary-cost-fleet-connections-canvas)).
