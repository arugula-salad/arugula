---
schema: 1
id: "aru-018"
title: "A block is a leaf with a type; a terminal is the first type"
state: "decided"
area: "The mux and panes"
source:
  via: "cli"
question: "A block is a leaf with a type; a terminal is the first type"
options:
  - id: "a"
    label: "A block is a leaf with a type; a terminal is the first type"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M6"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m6-non-terminal-blocks-after-m4b-m5-is-independent-of-it"
  - title: "Architecture"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#architecture"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/mux/src/block.rs"
  - "path:crates/mux/src/pane.rs"
  - "path:crates/mux/src/mux/blocks.rs"
---

# A block is a leaf with a type; a terminal is the first type

Every leaf of the tree is a block with a `type`. A terminal keeps its own fast
path (PTY bytes, offsets, snapshots). Every other type implements `Block`:
config saved in `layout.json`, JSON state pushed to clients, attention through
the same notices as terminals (so badges, "needs you" and push work for all of
them), a text rendering for `capture`/history/search, and methods. A pane
(`%N`) is a terminal block; OSC 133 command ranges are *command marks*, never
"blocks".
Where: `crates/mux/src/block.rs`, `crates/mux/src/pane.rs`, `crates/mux/src/mux/blocks.rs`.
From: [M6](docs/plan-archive.md#m6-non-terminal-blocks-after-m4b-m5-is-independent-of-it), [Architecture](docs/plan-archive.md#architecture).
