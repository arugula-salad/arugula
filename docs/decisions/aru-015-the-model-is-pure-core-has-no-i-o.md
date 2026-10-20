---
schema: 1
id: "aru-015"
title: "The model is pure: `core` has no I/O"
state: "decided"
area: "The mux and panes"
source:
  via: "cli"
question: "The model is pure: `core` has no I/O"
options:
  - id: "a"
    label: "The model is pure: `core` has no I/O"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Architecture"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#architecture"
  - title: "M1"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m1-multiplexer"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/core"
  - "path:crates/core/src/mux.rs"
  - "path:crates/core/src/layout.rs"
  - "path:crates/core/src/tree.rs"
---

# The model is pure: `core` has no I/O

Sessions hold tabs, tabs hold a split tree of panes. Clients change them by
sending intents, applied by pure functions in `crates/core`, and the tree
stays valid (ratios sum to 1, no empty splits). IDs follow tmux (`$session`,
`@tab`, `%pane`) and are never reused. Ratios are stored; cells are derived.
There are no local-only panes: every terminal belongs to some daemon.
Where: `crates/core/src/mux.rs`, `crates/core/src/layout.rs`, `crates/core/src/tree.rs`.
From: [Architecture](docs/plan-archive.md#architecture), [M1](docs/plan-archive.md#m1-multiplexer).
