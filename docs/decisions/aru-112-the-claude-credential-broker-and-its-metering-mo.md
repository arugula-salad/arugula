---
schema: 1
id: "aru-112"
title: "The Claude credential broker and its metering move to the platform"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1337"
question: "Where do studio's Claude credential broker and its metering live, once there's one platform?"
options:
  - id: "a"
    label: "The platform: one inference broker on step 5's caps and metering, which studio's lobby calls"
    how: "A project's members, its hosted workspaces and studio's lobby get Claude access through the platform, with the project's own key or the platform's key under the allowlist caps. The key never reaches a box or a browser. Spend is metered per project and per account on step 5's tables, and one place shows it. The lobby's rules are ported, so nobody's limits change on the day."
    tradeoff: "The lobby depends on the platform for every Claude call, and the platform carries studio's spend rules."
  - id: "b"
    label: "studio keeps its broker and reports spend to the platform"
    how: "The lobby keeps brokering and metering, and sends usage to the platform for one view."
    tradeoff: "Two brokers and two sets of caps for the same person's Claude spend."
choice:
  option: "a"
  reason: "One place holds the keys and shows Claude spend, and step 5 already built the caps and metering (Jake, 2026-10-22, platform step 8, deciding studio's part on Alex's behalf, pending Alex's review). The planner recommended b; Jake chose a."
rejected:
  - option: "b"
    why: "Two brokers for one person's spend."
supersedes: []
evidence:
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
  - title: "Arugula #22"
    url: "https://app.arugula.io/issues/iss_69d1c528120426c6b9a776435c9a10d6"
  - title: "Arugula #29"
    url: "https://app.arugula.io/issues/iss_77e700f2a960266a41118b5f4416ae4c"
  - title: "#1246"
    url: "https://github.com/arugula-salad/arugula/issues/1246"
  - title: "#1249"
    url: "https://github.com/arugula-salad/arugula/issues/1249"
decided_by: "jhgaylor"
decided_on: "2026-10-22"
reviews: []
constrains:
  - "path:crates/platform/src/hosted/inference.rs"
  - "path:crates/platform/src/hosted/usage.rs"
---

# The Claude credential broker and its metering move to the platform

studio's Claude credential broker, and its metering, move to the platform
(Jake, 2026-10-22, platform step 8, #1337). They join step 5's inference
caps and metering ("Hosted inference: strangers bring their own key",
amended).

- **One broker.** A project's members, its hosted workspaces and studio's
  lobby get Claude access through the platform, with the project's own key or
  the platform's key under the allowlist caps.
- **The key never reaches a box or a browser.**
- **Metering per project and per account,** on step 5's tables, and one
  place shows Claude spend.
- **studio's lobby calls it** instead of running its own broker. The lobby's
  rules are ported, so nobody's limits change on the day.

This differs from the planner's recommendation, which kept studio's broker
and reported its spend (option b); Jake chose to move it. The lobby's part
is Alex's: Jake decided it on Alex's behalf, pending Alex's review of this
record. If Alex declines in review, it goes back to Jake as a choice.
Where: the broker (Arugula #29, M231, native), `crates/platform/src/hosted/inference.rs`, `crates/platform/src/hosted/usage.rs`, studio's lobby (Arugula #34, M236).
From: #1337 (platform step 8, tracked natively in Arugula #22), M185 (#1246), #1249.
