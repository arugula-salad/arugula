---
schema: 1
id: "aru-096"
title: "Hosted inference: strangers bring their own key"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1236"
question: "Whose inference key do agents on hosted machines use, and how is spend bounded?"
options:
  - id: "a"
    label: "Strangers bring their own key; Jake, teams and the allowlist use the platform's key under caps; keys never enter the box"
    how: "A stranger's Anthropic or OpenAI key is a project secret, shown as set or unset and never read back, sealed by the platform and written into each box's egress rules. Allowlisted accounts run on the platform's dedicated keys, which have hard monthly spend limits at the provider, with per-account turn caps for fairness. The provider's egress proxy sets the key, so it's never in the box's environment or disk."
    tradeoff: "Strangers need a key of their own before an agent runs."
  - id: "b"
    label: "The platform's key for everyone"
    how: "Arugula pays for all hosted inference."
    tradeoff: "Spend can't be bounded for strangers."
  - id: "c"
    label: "Keys in the box's environment"
    how: "Pass the key as an env var at create."
    tradeoff: "Anything in the box can read and leak it."
choice:
  option: "a"
  reason: "Inference spend has to be bounded before strangers get hosted machines, and a key in the box is a key anyone in it can read (Jake, 2026-10-21, #1235)."
rejected:
  - option: "b"
    why: "Unbounded spend."
  - option: "c"
    why: "Keys never enter the box."
supersedes: []
evidence:
  - title: "#1235"
    url: "https://github.com/arugula-salad/arugula/issues/1235"
  - title: "#1236"
    url: "https://github.com/arugula-salad/arugula/issues/1236"
  - title: "#1246"
    url: "https://github.com/arugula-salad/arugula/issues/1246"
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src/hosted/inference.rs"
  - "path:crates/daemon/src/agent/hosted.rs"
---

# Hosted inference: strangers bring their own key

Agents on hosted machines need an inference key (Jake, 2026-10-21, #1235).

- **Strangers bring their own key.** Each one is a project secret (Anthropic
  or OpenAI), shown as set or unset and never read back.
- **Jake, teams and the allowlist** run on the platform's key, for
  workspaces an allowlisted account made (the maker pays). **The spend bound
  is the provider's** (Jake, 2026-10-21): the platform's keys are their own
  Anthropic workspace key and OpenAI project key, each with a hard monthly
  spend limit set at the provider. Caps per account on agent turns (at once,
  per day, minutes per day; 2, 100 and 120 by default) are fairness between
  accounts on top.
- **Keys never enter the box.** E2B's egress proxy sets the key on the box's
  requests to the provider; it's never in the box's environment or on its
  disk. The platform's keys are E2B secrets the operator keeps (Infisical
  keeps the record); a project's own key is kept sealed by the platform and
  written into each of its boxes' egress rules at create, rebuild, wake and
  rotation (Jake, 2026-10-21), so no account-wide cap on E2B secrets
  applies.
- **A workspace without a key,** whose owner isn't on the allowlist, says so
  when an agent starts, with a link to add one.

Release notes and the privacy page say whose key pays.

**Inference covers studio's broker** (step 8, #1337). studio's Claude
credential broker and its metering move to the platform, on these keys,
caps and tables, and studio's lobby calls it instead of brokering itself;
the lobby's rules are ported, so nobody's limits change on the day ("The
Claude credential broker and its metering move to the platform").
Where: `crates/platform/src/hosted/inference.rs` (whose key pays, project
keys, caps, the box's egress rules), `crates/platform/src/compute/` (egress
rules, E2B's `${e2b.secrets.<name>}` for the platform's keys),
`crates/daemon/src/agent/hosted.rs` (the box's placeholder key and the ask
before each turn), docs/control-ops.md (making the platform's keys and
their limits) (M185). loam's E2B API has no secrets broker yet
(arugula-salad/loam#8).
From: #1235 (platform roadmap, step 5), M175 (#1236), M185 (#1246), #1337 (platform step 8).
