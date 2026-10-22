---
schema: 1
id: "aru-093"
title: "The substrate is built to E2B's API"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1236"
question: "Which provider runs hosted machines, and how do we keep the option to run them ourselves?"
options:
  - id: "a"
    label: "E2B direct, with loam behind the same adapter until a dedicated loam cluster reaches parity"
    how: "One trait shaped after E2B's sandbox API (create, resume, pause, delete, status, list by metadata, hold) and one adapter that drives E2B's cloud and loam's E2B-compatible API, with only the base URL and key changed. A parity list says when loam can take over on a dedicated cluster."
    tradeoff: "We depend on one vendor's API shape, and loam has to keep up with it."
  - id: "b"
    label: "Managoat as the substrate"
    how: "Hosted machines run as Managoat workspaces."
    tradeoff: "Managoat is a conversation runner with a read-only sandbox API: no PTY, ports or resident processes."
  - id: "c"
    label: "An adapter per provider now (E2B, Sprites, Fly, Daytona)"
    how: "A provider-neutral trait with several adapters."
    tradeoff: "Code and tests for providers nobody uses (YAGNI)."
  - id: "d"
    label: "Self-host loam on our own machines now"
    how: "Run hosted workspaces on loam from day one."
    tradeoff: "loam doesn't yet have every feature we need, and geek must never run strangers' boxes."
choice:
  option: "a"
  reason: "E2B's pause keeps memory, a paused box costs nothing, and template snapshots start in about 80 ms; loam speaks the same API, so moving later is a base URL (Jake, 2026-10-21, #1235)."
rejected:
  - option: "b"
    why: "Managoat may have a role at another layer later, but not as this step's substrate; compute isn't split by job."
  - option: "c"
    why: "No adapter until something needs it."
  - option: "d"
    why: "loam is a target we can hit once it has parity, on a dedicated cluster, never on geek."
supersedes: []
evidence:
  - title: "#1235"
    url: "https://github.com/arugula-salad/arugula/issues/1235"
  - title: "#1236"
    url: "https://github.com/arugula-salad/arugula/issues/1236"
  - title: "#1237"
    url: "https://github.com/arugula-salad/arugula/issues/1237"
  - title: "#1238"
    url: "https://github.com/arugula-salad/arugula/issues/1238"
  - title: "#1252"
    url: "https://github.com/arugula-salad/arugula/issues/1252"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src/compute"
---

# The substrate is built to E2B's API

Hosted machines run on E2B's cloud, through a substrate built to E2B's API
(Jake, 2026-10-21, #1235).

- **One trait, one adapter.** `crates/platform/src/compute/` has a trait with
  exactly create (from a template, with env, metadata and a timeout; boxes
  pause on timeout, never die), resume, pause, delete, status (which never
  wakes a box), list by metadata, and hold (extend the timeout). One adapter
  drives E2B's cloud and loam's E2B-compatible API; only the base URL and the
  key change.
- **The provider's key is a platform secret.** It never reaches a client or
  a box.
- **loam is a target we can hit.** M176's suite runs against loam too, and the
  parity list (M191) records every gap. When loam supports every E2B feature
  we use, and E2B's bill makes it worth it, workspaces move to a dedicated
  loam cluster for Arugula. Where it runs is decided then, and it's never
  geek.
- **No other adapters** (Sprites, Fly, Daytona) until something needs one.
  Managoat isn't the substrate.

The daemon's own Labs providers (`crates/mux/src/provider`, `labs/loam.rs`)
are a different thing and stay as they are.
Where: `crates/platform/src/compute/` (M176), the template build (M177).
From: #1235 (platform roadmap, step 5), M175 (#1236), M176 (#1237), M177 (#1238), M191 (#1252).
