---
schema: 1
id: "aru-014"
title: "One task decides the order of every change"
state: "decided"
area: "The mux and panes"
source:
  via: "cli"
question: "One task decides the order of every change"
options:
  - id: "a"
    label: "One task decides the order of every change"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Cargo workspace"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#cargo-workspace"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/mux/src/mux/mod.rs"
  - "path:crates/mux/src/pane.rs"
---

# One task decides the order of every change

The mux task owns the layout, the panes and the connected clients; every
client message, API call and pane notice goes through it. A pane's VT thread
owns libghostty's terminal (it is `!Send`), and PTY output, attaches, resizes
and exits arrive on one channel, so a snapshot and the live output after it
are always in order and the log sees the same bytes.
Where: `crates/mux/src/mux/mod.rs`, `crates/mux/src/pane.rs`.
From: [Cargo workspace](docs/plan-archive.md#cargo-workspace), M1.
