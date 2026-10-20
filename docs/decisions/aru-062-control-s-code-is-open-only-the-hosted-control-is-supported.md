---
schema: 1
id: "aru-062"
title: "Control's code is open, only the hosted control is supported; the tailnet stays first class"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "Control's code is open, only the hosted control is supported; the tailnet stays first class"
options:
  - id: "a"
    label: "Control's code is open, only the hosted control is supported; the tailnet stays first class"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Control track"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#control-track-s15-m17m22-added-2026-10-01"
  - title: "#662"
    url: "https://github.com/arugula-salad/arugula/issues/662"
decided_by: "jhgaylor"
decided_on: "2026-10-08"
reviews: []
constrains:
  - "path:crates/control"
  - "path:packaging/control"
---

# Control's code is open, only the hosted control is supported; the tailnet stays first class

`arugula-control` lives in this repo and the hosted one
(control.arugula.io) runs the same code. For now only the hosted one is
supported: a control you run yourself gets no help (lex00, 2026-10-08).
People who want a setup of their own use the tailnet without control,
which stays first class: tailnet users skip control, or enroll and keep
direct connections. Free for one person, per seat for teams, sandboxes by
usage.
Where: `crates/control/`, `packaging/control/`.
From: [Control track](docs/plan-archive.md#control-track-s15-m17m22-added-2026-10-01), M22, #662.
