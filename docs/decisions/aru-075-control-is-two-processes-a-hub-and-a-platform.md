---
schema: 1
id: "aru-075"
title: "Control is two processes: a hub and a platform"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1151"
question: "How do we stop a chat, Cloud or billing deploy from restarting the process that holds every relayed terminal?"
options:
  - id: "a"
    label: "Two processes in one repo: a hub (identity, devices, rosters, relay, push, origins, jump host, forge, telemetry) and a platform (chat, Cloud, billing, projects, grants), joined by an additive HubPort"
    how: "crates/platform holds the platform's code; HubPort is the one interface it calls the hub through, implemented in process first and over HTTP once the platform is its own Fly app. Additive only: we deploy both sides, platform first. In files the process is the key process = \"hub\" | \"platform\" (routes.toml, tables.toml), never plane."
    tradeoff: "A second Fly app to deploy and watch, and every platform call goes through the hub's gateway."
  - id: "b"
    label: "Keep one process and make restarts cheaper"
    how: "Hand relay sockets over across a restart, or run a second instance of the whole of control."
    tradeoff: "The relay, passkey challenges, rate limits and the replay cache live in one in-memory App, so a second instance can't run; a platform bug still takes terminals down with it."
  - id: "c"
    label: "Split the repo: the platform as its own codebase"
    how: "A separate repository and release train for the platform."
    tradeoff: "Two release trains and a wire contract across repos before the seam has settled."
choice:
  option: "a"
  reason: "Today a chat fix restarts the process holding every relayed terminal; with two processes the hub deploys rarely and the platform as often as it likes, and one repo keeps the seam cheap to change while it settles (Jake, 2026-10-20, #1150)."
rejected:
  - option: "b"
    why: "Control's state is one in-memory App; nothing short of a split lets the platform restart alone."
  - option: "c"
    why: "Rejected in the platform planes review: a process boundary gets the deploy isolation, a repo boundary only adds release coordination."
supersedes: []
evidence:
  - title: "#1150"
    url: "https://github.com/arugula-salad/arugula/issues/1150"
  - title: "#1151"
    url: "https://github.com/arugula-salad/arugula/issues/1151"
  - title: "#1153"
    url: "https://github.com/arugula-salad/arugula/issues/1153"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:crates/control/src/main.rs"
  - "path:crates/platform"
  - "path:crates/control/tables.toml"
  - "path:crates/control/routes.toml"
---

# Control is two processes: a hub and a platform

Control runs as two processes (Jake, 2026-10-20, #1150):
- **The hub** keeps identity (sessions, passkeys, GitHub sign-in), devices,
  rosters, the relay, push and its VAPID key, block and doc origins (Shared
  Docs and doc links stay here), the ssh jump host, forge and telemetry. It
  holds every relayed terminal, so it deploys rarely.
- **The platform** keeps chat, Cloud, billing, and step 1's projects and
  grants. It deploys as often as it likes, and a deploy or a bug there
  doesn't drop a terminal.

The seam is `HubPort`, the one interface the platform calls the hub
through (who someone is, what devices and rosters say, push). It's
implemented in process first (M136) and over the private network once the
platform is its own Fly app (M140). It only grows: we deploy both sides,
platform first, so a new platform never calls a hub method that isn't there
yet. If `HubPort` changes shape week after week, the seam is in the wrong
place, and we stop and rethink (#1150).

**Processes are not planes.** "Two planes, two trust rules" names trust
planes: what control may read (machine) and what it holds in plaintext
because it has to exist while machines sleep (platform). The hub and the
platform are processes, and the two don't line up exactly: Shared Docs are
platform-plane data the hub process holds, and the hub's relay, devices
and rosters are the machine plane. In files, the process is the key
`process = "hub" | "platform"` (`routes.toml`, `tables.toml`), never
`plane`, which keeps meaning the trust plane.

One repo, not two: the platform is `crates/platform` beside `crates/control`
(rejected in the platform planes review: a repo boundary adds release
coordination, not isolation).
Where: `crates/control/src/main.rs` (`App`), `crates/platform/`, `crates/control/routes.toml`, `crates/control/tables.toml`.
From: #1150 (platform roadmap, step 2), M134 (#1151), M136 (#1153).
