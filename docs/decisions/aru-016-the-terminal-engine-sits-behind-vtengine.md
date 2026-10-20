---
schema: 1
id: "aru-016"
title: "The terminal engine sits behind `VtEngine`"
state: "decided"
area: "The mux and panes"
source:
  via: "cli"
question: "The terminal engine sits behind `VtEngine`"
options:
  - id: "a"
    label: "The terminal engine sits behind `VtEngine`"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Decisions table"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#decisions-on-the-briefs-open-questions"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/vt/src/lib.rs"
  - "path:crates/vt/src/ghostty.rs"
---

# The terminal engine sits behind `VtEngine`

libghostty-vt is the only engine, behind a trait (`feed`, `resize`,
`snapshot`, …) so the daemon never touches Ghostty types directly. The Ghostty
commit is pinned.
Why: it ships a formatter that turns state back into VT bytes and keeps more
state than xterm's serializer.
Where: `crates/vt/src/lib.rs`, `crates/vt/src/ghostty.rs`.
From: [Decisions table](docs/plan-archive.md#decisions-on-the-briefs-open-questions), S1.
