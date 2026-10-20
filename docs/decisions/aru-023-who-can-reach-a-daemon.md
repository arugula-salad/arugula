---
schema: 1
id: "aru-023"
title: "Who can reach a daemon"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
question: "Who can reach a daemon"
options:
  - id: "a"
    label: "Who can reach a daemon"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Architecture"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#architecture"
  - title: "M4"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/daemon/src/access.rs"
  - "path:crates/daemon/src/tailscale.rs"
  - "path:crates/daemon/src/localauth.rs"
---

# Who can reach a daemon

The daemon listens on loopback; `tailscale serve` adds `Tailscale-User-Login`
and strips any copy a client sends, and direct tailnet connections are
identified by asking tailscaled (WhoIs). Host must be a loopback or tailnet
name (DNS rebinding). The Unix socket maps the peer's uid to the owner. A
device of the control account the daemon joined is the owner too.
Where: `crates/daemon/src/access.rs`, `crates/daemon/src/tailscale.rs`, `crates/daemon/src/localauth.rs`.
From: [Architecture](docs/plan-archive.md#architecture), [M4](docs/plan-archive.md#m4-reach-a-shell-on-any-machine-or-sandbox), S2.
