---
schema: 1
id: "aru-142"
title: "Control is one process again"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "With device keys gone (aru-141), does control still run as two processes, a hub and a platform?"
options:
  - id: "a"
    label: "One process, before the launch"
    how: "The hub and the platform become one process and one Fly app. The gateway's signed principal header becomes an in-process call, and every path keeps its address."
    tradeoff: "A deploy of anything drops every relayed terminal connection for a moment; clients and daemons reconnect, and the terminals themselves live on the daemons."
  - id: "b"
    label: "Keep two processes, the hub shrunk to the relay, gateway and sign-in"
    how: "Devices, rosters and trust leave the hub; everything else that isn't a long-lived connection moves to the platform over time."
    tradeoff: "Two deploys, the HubPort seam and the gateway kept, to save a reconnect on deploy."
choice:
  option: "a"
  reason: "Two processes kept two things apart: the keys' trust from the platform, and the relay's connections from frequent deploys (aru-075). aru-141 removes the first. A reconnect blip on deploy is acceptable (Jake, 2026-11-13), so the second doesn't justify two deploys and a seam. The inversion's one-app work (#989) then ships as one deploy instead of two."
rejected:
  - option: "b"
    why: "It keeps a seam whose remaining reason Jake accepted losing."
supersedes:
  - decision: "aru-075"
evidence:
  - title: "#agents, 2026-11-13, the thread of seq 61 (seq 67)"
  - title: "Board: The inversion: machines become a component (brd_d08cd00433b2b2a052ba37b7899d3785, #989)"
decided_by: "jhgaylor"
decided_on: "2026-11-13"
reviews: []
constrains:
  - "aru-076"
  - "aru-080"
  - "aru-081"
  - "aru-141"
---

# Control is one process again

Control has run as two processes since step 2 of the platform roadmap
(aru-075): a hub holding identity, devices, rosters and the relay, which
deployed rarely, and a platform, which deployed freely. With device keys gone
(aru-141), nothing about trust keeps them apart, and Jake accepts a reconnect
blip on deploy (2026-11-13). Decided, before the launch on Dec 1:

- **One process and one Fly app** serve everything control does: the relay,
  sign-in and the issuer (aru-106), push, the platform's routes and the one
  app's page.
- **Paths keep their addresses.** aru-076's rule stands, that no route an old
  daemon or app calls moves, and so does `control.arugula.io` as the passkeys'
  relying party (aru-078). The gateway's HMAC principal header becomes an
  in-process call; old daemons' signed requests are checked as before.
- **A deploy drops relayed connections for a moment.** Clients and daemons
  reconnect; panes live on the daemons and don't notice.
- **Left to whoever builds it:** the databases (the hub's SQLite and the
  platform's Postgres, aru-077), the order of the move, and how `tables.toml`
  and `routes.toml` record what used to be `process`. Amend this record with
  what they choose. Until someone takes it in #agents, the merge has no owner.

Where: the hub and the platform are in arugula-salad/cloud.
