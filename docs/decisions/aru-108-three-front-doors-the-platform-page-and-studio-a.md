---
schema: 1
id: "aru-108"
title: "Three front doors: the platform page, and studio and hud as clients"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1337"
question: "Which of the org's seven front doors survive once there's one platform?"
options:
  - id: "a"
    label: "The platform page, studio as a client and hud as a client; planter, redone and experimental's control plane retire"
    how: "app.arugula.io is the platform's own page. studio and hud keep their front doors, sign people in through the hub and read projects from the platform. planter is absorbed: its accounts and repos import, its deploys become the planter driver, and its GitHub App stays the deploy App. redone's tenants become projects, its tokens agent grants and its approvals device-signed deploys. experimental's control plane is never built. Retired domains redirect for a year, and DNS is never reused."
    tradeoff: "planter's and redone's users move, and their old tokens end on announced dates."
  - id: "b"
    label: "Keep planter and redone as products, signed in through the hub"
    how: "One account, but planter and redone keep their own projects, deploys and secrets."
    tradeoff: "Two more places holding what the platform's projects, drivers and deploys already hold."
  - id: "c"
    label: "Fold studio and hud into the platform page"
    how: "One page for everything."
    tradeoff: "studio's lobby and hud's boxes are apps of their own with owners of their own; one account doesn't need one page."
choice:
  option: "a"
  reason: "planter and redone do what the platform's projects, drivers and deploys now do; studio and hud do things the platform doesn't, so they stay and become clients (Jake, 2026-10-22, platform step 8, including studio's and hud's parts on Alex's and Aaron's behalf, pending their review)."
rejected:
  - option: "b"
    why: "Their work is the platform's now."
  - option: "c"
    why: "studio and hud stay their owners' apps, as clients."
supersedes: []
evidence:
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
  - title: "Arugula #22"
    url: "https://app.arugula.io/issues/iss_69d1c528120426c6b9a776435c9a10d6"
  - title: "Arugula #32"
    url: "https://app.arugula.io/issues/iss_868a4cd19055b425d39174c036ba362b"
  - title: "Arugula #33"
    url: "https://app.arugula.io/issues/iss_fafeedc287db9ef4e5e9bfe05ee12ade"
  - title: "Arugula #37"
    url: "https://app.arugula.io/issues/iss_d4a034fcd958cd354ed0493623c3d88a"
decided_by: "jhgaylor"
decided_on: "2026-10-22"
reviews: []
constrains:
  - "path:crates/control/src/platform_host.rs"
  - "path:crates/platform/src/drivers"
  - "path:docs/control-ops.md"
---

# Three front doors: the platform page, and studio and hud as clients

Seven front doors become three (Jake, 2026-10-22, platform step 8, #1337).

**They survive:**
- **The platform page,** `app.arugula.io` ("The platform page is on
  app.arugula.io, not on the keys' origin").
- **studio, as a client** ("studio is a full client: Sign in with Arugula,
  apps by project").
- **hud, as a client** ("hud is an in-app client: platform assertions, a
  project roster").

**They retire:**
- **planter.** Its accounts and repos import into the platform, and its
  deploys become the planter driver ("A driver is a run with a credential").
  planter's GitHub App stays the deploy App; the platform's own App gets no
  more permissions. `planter.arugula.io` redirects, and the ship API stays
  as a shim for 90 days.
- **redone.** Tenants become projects, `rdn_` tokens become agent grants on
  the hosted MCP endpoint ("A hosted MCP endpoint serves agents with no
  machine"), approver keys become device-signed approvals, and plan, approve,
  apply becomes a deploy.
- **experimental's control plane** ("arugula is the platform's home;
  experimental's D3 is retired").

**Domains go last.** Every retired domain redirects for a year, its end date
is written in `docs/control-ops.md`, and its DNS is never reused. Old tokens
and shims end on announced dates, each one asked of Jake first.
Where: the platform page (`crates/control/src/platform_host.rs`), the drivers (`crates/platform/src/drivers`), `docs/control-ops.md` (the redirects), planter (Arugula #30–#32, M232–M234), redone (Arugula #33, M235), domains (Arugula #37, M239, native).
From: #1337 (platform step 8, tracked natively in Arugula #22).
