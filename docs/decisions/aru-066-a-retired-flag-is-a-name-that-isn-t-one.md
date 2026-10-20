---
schema: 1
id: "aru-066"
title: "A retired flag is a name that isn't one"
state: "decided"
area: "Build and release"
source:
  via: "cli"
question: "A retired flag is a name that isn't one"
options:
  - id: "a"
    label: "A retired flag is a name that isn't one"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#942"
    url: "https://github.com/arugula-salad/arugula/issues/942"
decided_by: "jhgaylor"
decided_on: "2026-10-18"
reviews: []
constrains:
  - "path:crates/proto/src/flags.rs"
  - "path:crates/daemon/tests/integration/flag_gates.rs"
---

# A retired flag is a name that isn't one

A Labs feature that stops being a flag takes its row out of `FLAGS` and
puts its name in `flags::RETIRED`, with why. `fountain` was the first
(#942's F5, when Fountain left Labs; its daemon side was later removed:
"Daemon-side Managoat is removed", under Agents and blocks), with
`managoat`, the name it would have now, beside it. A `flags.json` or a
control push that names one, on or off, is read as any unknown name always
was: ignored, and kept by a write, so an older daemon reading the same file
keeps its word. `arugulad flags NAME …` and `PUT /api/flags/NAME` say why
it's gone instead of "no flag named".
Where: `crates/proto/src/flags.rs` (`RETIRED`, `retired`), `crates/daemon/tests/integration/flag_gates.rs`.
From: #942 (Q4), F5.
