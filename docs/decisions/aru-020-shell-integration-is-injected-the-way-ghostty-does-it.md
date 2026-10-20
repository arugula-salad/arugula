---
schema: 1
id: "aru-020"
title: "Shell integration is injected the way Ghostty does it"
state: "decided"
area: "The mux and panes"
source:
  via: "cli"
question: "Shell integration is injected the way Ghostty does it"
options:
  - id: "a"
    label: "Shell integration is injected the way Ghostty does it"
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
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/mux/src/shellint.rs"
  - "path:crates/mux/src/osc.rs"
---

# Shell integration is injected the way Ghostty does it

bash `ENV`, zsh `ZDOTDIR` and fish `XDG_DATA_DIRS` load scripts that report
prompts, commands, exit codes and the directory (OSC 133, 7, 633), without
touching the user's dotfiles; a pane can switch it off.
Where: `crates/mux/src/shellint.rs`, `crates/mux/src/osc.rs`.
From: [M3](docs/plan-archive.md#m3-structure-and-cli).
