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
    how: "A stranger's Anthropic or OpenAI key is a project secret, shown as set or unset and never read back. Allowlisted accounts run on the platform's key under the ARUGULA_CLOUD_* caps (concurrent runs, daily runs, run minutes) per account. The provider's secrets broker injects the key at its egress proxy, so it's never in the box's environment or disk."
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
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src/cloud/account.rs"
  - "path:crates/platform/src/cloud/secrets.rs"
---

# Hosted inference: strangers bring their own key

Agents on hosted machines need an inference key (Jake, 2026-10-21, #1235).

- **Strangers bring their own key.** Each one is a project secret (Anthropic
  or OpenAI), shown as set or unset and never read back.
- **Jake, teams and the allowlist** run on the platform's key, under the
  `ARUGULA_CLOUD_*` caps (concurrent runs, daily runs, run minutes) per
  account.
- **Keys never enter the box.** The provider's secrets broker injects the key
  at its egress proxy; it's never in the box's environment or on its disk.
- **A workspace without a key,** whose owner isn't on the allowlist, says so
  when an agent starts, with a link to add one.

Release notes and the privacy page say whose key pays.
Where: `crates/platform/src/cloud/account.rs` (the allowlist and caps), project secrets (M185).
From: #1235 (platform roadmap, step 5), M175 (#1236), M185 (#1246).
