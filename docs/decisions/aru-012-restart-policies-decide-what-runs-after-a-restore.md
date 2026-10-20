---
schema: 1
id: "aru-012"
title: "Restart policies decide what runs after a restore"
state: "decided"
area: "State and durability"
source:
  via: "cli"
question: "Restart policies decide what runs after a restore"
options:
  - id: "a"
    label: "Restart policies decide what runs after a restore"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Restart policies"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#restart-policies-m2"
  - title: "Restoring scrollback"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#restoring-scrollback"
  - title: "#146"
    url: "https://github.com/arugula-salad/arugula/issues/146"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/proto/src/lib.rs"
  - "path:crates/mux/src/resume.rs"
  - "path:crates/mux/src/store.rs"
---

# Restart policies decide what runs after a restore

Scrollback always comes back. A pane's policy decides what runs in it: `none`
(press Enter for a shell), `shell` (the default: a login shell in the last
OSC 7 directory), `rerun` (the foreground command, asking first by default),
`hook` (a stored command), `resume` (the agent conversation that was running:
`claude --resume <id>`; the default for a pane running Claude Code). Restoring
feeds a fresh engine the checkpoint and log tail, then writes a reset and a
dim "restored" rule before the new process starts.
Where: `crates/proto/src/lib.rs` (`Policy`), `crates/mux/src/resume.rs`, `crates/mux/src/store.rs`.
From: [Restart policies](docs/plan-archive.md#restart-policies-m2), [Restoring scrollback](docs/plan-archive.md#restoring-scrollback), #146.
