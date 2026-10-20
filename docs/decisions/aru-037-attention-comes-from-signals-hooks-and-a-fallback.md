---
schema: 1
id: "aru-037"
title: "Attention comes from signals, hooks and a fallback"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "Attention comes from signals, hooks and a fallback"
options:
  - id: "a"
    label: "Attention comes from signals, hooks and a fallback"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M3"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m3-structure-and-cli"
  - title: "M29"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m29-team-answers"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/mux/src/mux/attention.rs"
  - "path:crates/mux/src/status.rs"
  - "path:crates/cli/src/hook.rs"
  - "path:crates/daemon/src/push.rs"
---

# Attention comes from signals, hooks and a fallback

Each pane is `idle | working | needs-input | done`, from notification
sequences (OSC 9/777/99, BEL), Claude Code hooks (`illogical hook`, which also
turns a permission request into an approval card), and a quiet-output
heuristic. A program that reports its status (OSC 7501, #720) beats the
screen rules and the quiet heuristic for its pane; hooks and open asks still
beat it, since they carry the question and how to answer it. Every answer has an author. Web Push goes to the phone when no
client is focused on the pane.
Where: `crates/mux/src/mux/attention.rs`, `crates/mux/src/status.rs`, `crates/cli/src/hook.rs`, `crates/daemon/src/push.rs`.
From: [M3](docs/plan-archive.md#m3-structure-and-cli), [M29](docs/plan-archive.md#m29-team-answers), M24.
