---
schema: 1
id: "aru-030"
title: "The web client draws terminals through one file"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "The web client draws terminals through one file"
options:
  - id: "a"
    label: "The web client draws terminals through one file"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M1"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m1-multiplexer"
  - title: "M8"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m8-client-terminal-engine-ghostty-web-and-local-echo"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:web/src/terminal-view.ts"
---

# The web client draws terminals through one file

xterm.js lives behind `terminal-view.ts` (WebGL on visible panes, DOM for the
rest) so the renderer can be swapped later (M8, gated, not built). Layout
changes are intents sent to the daemon and never applied locally.
Where: `web/src/terminal-view.ts`.
From: [M1](docs/plan-archive.md#m1-multiplexer), [M8](docs/plan-archive.md#m8-client-terminal-engine-ghostty-web-and-local-echo).
