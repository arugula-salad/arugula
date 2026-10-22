---
schema: 1
id: "aru-094"
title: "Hosted machines pause only between turns"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1236"
question: "When may a hosted machine be paused?"
options:
  - id: "a"
    label: "Only between turns: hold the box while its daemon reports busy, let it pause when idle"
    how: "The daemon reports busy from the start of an agent turn or a foreground command to its end. While busy, the platform keeps extending the sandbox's timeout; when idle, the timeout runs out and the box pauses with memory and disk kept. A silent daemon pauses after the idle timeout."
    tradeoff: "An unwatched turn runs (and costs) until it ends."
  - id: "b"
    label: "When nobody is watching"
    how: "Pause as soon as the last viewer leaves."
    tradeoff: "Freezes unwatched agent turns, which are the normal case for hosted agents."
  - id: "c"
    label: "Never pause"
    how: "Keep boxes running."
    tradeoff: "Pays for idle boxes around the clock."
choice:
  option: "a"
  reason: "An unwatched agent turn is the normal case for hosted agents; freezing it mid-turn breaks the product (Jake, 2026-10-20 and 2026-10-21, #1235)."
rejected:
  - option: "b"
    why: "Breaks unwatched turns."
  - option: "c"
    why: "Cost without use."
supersedes: []
evidence:
  - title: "#1235"
    url: "https://github.com/arugula-salad/arugula/issues/1235"
  - title: "#1236"
    url: "https://github.com/arugula-salad/arugula/issues/1236"
  - title: "#1241"
    url: "https://github.com/arugula-salad/arugula/issues/1241"
  - title: "#1242"
    url: "https://github.com/arugula-salad/arugula/issues/1242"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src/compute"
  - "path:crates/daemon/src/agent"
---

# Hosted machines pause only between turns

A hosted machine pauses only between turns (Jake, 2026-10-20 and 2026-10-21,
#1235). A turn nobody is watching runs to its end, then the box sleeps.

- **Busy and idle come from the daemon.** It's busy from the start of an
  agent turn or a foreground command to its end, and reports it over its
  control link (M180).
- **The platform holds a busy box** by extending the sandbox's timeout. When
  the daemon reports idle, the timeout runs out and the provider pauses the
  box, keeping memory and disk.
- **Nothing holds a box forever.** A daemon that goes silent is treated as
  idle, so the box pauses after the normal idle timeout.
- **E2B's cap on continuous runtime** (24 hours on Pro): before a busy box
  reaches it, the platform pauses and resumes it at the next idle moment.
  A single turn longer than the cap is reported in its transcript, and to
  Jake.

Never pause a box whose daemon reports busy.
Where: `crates/platform/src/compute/` (the hold, M181), the daemon's busy/idle report (M180).
From: #1235 (platform roadmap, step 5), M175 (#1236), M180 (#1241), M181 (#1242).
