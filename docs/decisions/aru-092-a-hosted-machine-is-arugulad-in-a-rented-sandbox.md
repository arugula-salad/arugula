---
schema: 1
id: "aru-092"
title: "A hosted machine is arugulad in a rented sandbox"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1236"
question: "What is a hosted Cloud workspace: a machine of ours in someone's VM, or something the platform drives itself?"
options:
  - id: "a"
    label: "arugulad in a rented sandbox, dialing out to the relay; the provider handles only the lifecycle"
    how: "The box runs the same arugulad as a laptop, in hosted mode. It enrolls into a project, dials the relay, and brings terminals, agent blocks and previews itself. The platform creates, wakes, holds and pauses the box through the provider, and the hub wakes it when someone opens it."
    tradeoff: "A daemon release reaches hosted boxes only through a new template, and a paused box must be woken before anyone sees it."
  - id: "b"
    label: "Daemon-less: the platform drives the provider's PTY and exec APIs"
    how: "The platform opens terminals and runs agents in the sandbox itself, over the provider's APIs."
    tradeoff: "A second multiplexer in the platform, about 15–25k lines, and control would read terminals."
  - id: "c"
    label: "Keep Managoat conversations as Cloud workspaces"
    how: "As #1039 built it: a Cloud workspace is a Managoat environment whose tabs are conversations."
    tradeoff: "No terminal, no ports, no resident processes, account-only, and the daemon reaches it through proxies."
choice:
  option: "a"
  reason: "One compute unit with one trust story: a hosted machine is the same arugulad as on a laptop, so terminals, agents and previews come through it end to end (Jake, 2026-10-21, #1235)."
rejected:
  - option: "b"
    why: "Control would read terminals, against \"Two planes, two trust rules\", and the platform would grow a second multiplexer."
  - option: "c"
    why: "Managoat has no terminal, ports or resident processes, and parks after an hour without a turn of its own."
supersedes: []
evidence:
  - title: "#1235"
    url: "https://github.com/arugula-salad/arugula/issues/1235"
  - title: "#1236"
    url: "https://github.com/arugula-salad/arugula/issues/1236"
  - title: "#1240"
    url: "https://github.com/arugula-salad/arugula/issues/1240"
  - title: "#1241"
    url: "https://github.com/arugula-salad/arugula/issues/1241"
  - title: "#1243"
    url: "https://github.com/arugula-salad/arugula/issues/1243"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src/compute"
  - "path:crates/daemon/src/main.rs"
  - "path:crates/platform/src/cloud"
---

# A hosted machine is arugulad in a rented sandbox

A hosted Cloud workspace is a machine: `arugulad` running in a rented sandbox
(Jake, 2026-10-21, #1235). It's the same daemon as on a laptop, started with
`--hosted`.

- **It dials out.** The box enrolls into its project with a one-time ticket
  ("Control enrolls hosted machines under a project delegation") and dials the
  relay like any machine. Terminals, agent blocks and previews come through
  it end to end. Nothing reaches into the box from the platform.
- **The provider handles only the lifecycle:** create from a template, wake,
  hold (extend the timeout), pause and delete. No exec, files or ports go
  through the provider ("The substrate is built to E2B's API").
- **The hub wakes it.** Opening a paused workspace from a terminal, a block,
  the phone or MCP makes the hub ask the platform to resume the box, and the
  daemon redials at once.

Daemon-less designs were rejected: the platform would be a second
multiplexer (about 15–25k lines), and control would read terminals. This
retires the M20 hosted-sandbox product (`arugulad` in a Sprites VM, joined
with a one-time ticket) and Managoat as the Cloud workspace backend
("Cloud execution is owned by Arugula control", amended). Both are removed
in M189 and M190.
Where: `crates/platform/src/compute/` (M176), `crates/daemon/` (`--hosted`, M180), `crates/platform/src/cloud/` (until M189).
From: #1235 (platform roadmap, step 5), M175 (#1236), M179 (#1240), M180 (#1241), M182 (#1243).
