---
schema: 1
id: "aru-009"
title: "The log is the truth; checkpoints are a cache"
state: "decided"
area: "State and durability"
source:
  via: "cli"
question: "The log is the truth; checkpoints are a cache"
options:
  - id: "a"
    label: "The log is the truth; checkpoints are a cache"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "S5"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#s-spikes-each-about-half-a-day-before-the-milestone-named"
  - title: "M2"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m2-durability"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/mux/src/store.rs"
  - "path:crates/vt/src/ghostty.rs"
---

# The log is the truth; checkpoints are a cache

A pane's history is raw output in 4 MB segments with a sidecar index
(resizes, restores). A checkpoint (GHOSTSNP, zstd, tagged with the engine
that wrote it) speeds up restore, but one that doesn't decode is discarded
and the log tail replayed, because the format changed incompatibly once
without a version bump. Retention is 256 MB a pane.
Where: `crates/mux/src/store.rs` (`RETAIN_BYTES`, `PaneLog`), `crates/vt/src/ghostty.rs`.
From: [S5](docs/plan-archive.md#s-spikes-each-about-half-a-day-before-the-milestone-named), [M2](docs/plan-archive.md#m2-durability).
