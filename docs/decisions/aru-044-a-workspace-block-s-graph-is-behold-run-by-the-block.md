---
schema: 1
id: "aru-044"
title: "A workspace block's graph is behold, run by the block"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "A workspace block's graph is behold, run by the block"
options:
  - id: "a"
    label: "A workspace block's graph is behold, run by the block"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#620"
    url: "https://github.com/arugula-salad/arugula/issues/620"
  - title: "INTENTIUS/behold#474"
    url: "https://github.com/INTENTIUS/behold/issues/474"
decided_by: "jhgaylor"
decided_on: "2026-10-07"
reviews: []
constrains:
  - "path:crates/daemon/src/labs/workspace/graph.rs"
  - "path:web/src/blocks/workspace-graph.tsx"
---

# A workspace block's graph is behold, run by the block

The estate graph (resources, drift, the ops track) is behold's to draw; the
block doesn't read chant's `graph`. *Graph* on a member card (the owner's)
has the block start `behold serve <root> --port 0` on this host and frame it
through the block's own site, as an editor block frames code-server. It's
the Workspace block's process, not a Browser block beside it or a new block
type: a pick in behold has to reach this block (to open the member's Shell
or Changes), the block's env and fingerprint drive it, and its lifetime is
the block's. behold runs in its own process group, stopped as a group when
the block closes or the daemon stops.
behold gets the environment a Shell pane in the member gets (the user's
login shell over the daemon's), so it sees the credentials that shell has
and Arugula adds none. It's started with no `--env`, which would read live
at startup and on each change: the block's env reaches it through the frame,
`gates=<env>` for its gate strip, and the graph's env only when the person
asks for a live read. A later env change is a `behold:view` message
carrying `gates` (and `env` when live), so the frame never loads again.
behold listens on loopback and answers only its loopback names; the site
proxy hands it `Host: localhost:<port>` and its own
origin, so no `--allow-host` is needed. A settled read tells behold
(`POST /api/refresh?notify=1`), so behold doesn't poll the repo too. Framed,
behold draws gates without an approve button: the block approves (one
approve path per gate). `$ARUGULA_BEHOLD` picks the behold to run. This
host only for now.
Where: `crates/daemon/src/labs/workspace/graph.rs`, `web/src/blocks/workspace-graph.tsx`.
From: #620, INTENTIUS/behold#474.
