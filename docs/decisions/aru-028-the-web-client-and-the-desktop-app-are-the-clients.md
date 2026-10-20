---
schema: 1
id: "aru-028"
title: "The web client and the desktop app are the clients"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "The web client and the desktop app are the clients"
options:
  - id: "a"
    label: "The web client and the desktop app are the clients"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "S26"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#s26-how-native-can-it-get-141"
  - title: "#387"
    url: "https://github.com/arugula-salad/arugula/issues/387"
decided_by: "jhgaylor"
decided_on: "2026-10-18"
reviews: []
constrains:
  - "path:web"
  - "path:crates/desktop"
  - "path:crates/cli/src/attach.rs"
---

# The web client and the desktop app are the clients

Panes are drawn by the web client (in a browser, on the phone, and in the
desktop app's webview). The CLI scripts and drives panes (`run`, `send`,
`tail`, `wait`, `capture`) and `arugula attach` puts one pane in the terminal
you're in, but nothing else draws tabs and splits.
Why: every feature lands once. The TUI and the tmux `-CC` front end were
frozen first (#387) and then removed: "The TUI and tmux -CC are removed",
below.
Where: `web/`, `crates/desktop/`, `crates/cli/src/attach.rs`.
From: #387; [S26](docs/plan-archive.md#s26-how-native-can-it-get-141); Jake, 2026-10-18.
