---
schema: 1
id: "aru-055"
title: "Control can refuse service but can't read"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
question: "Control can refuse service but can't read"
options:
  - id: "a"
    label: "Control can refuse service but can't read"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Control track"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#control-track-s15-m17m22-added-2026-10-01"
  - title: "M18"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m18-relay-and-end-to-end-encryption"
  - title: "docs/control-e2e.md"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/control-e2e.md"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/e2e/src/channel.rs"
  - "path:crates/control/src/relay.rs"
  - "path:crates/daemon/src/e2e.rs"
---

# Control can refuse service but can't read

Terminal output, input, scrollback, snapshots and push payloads are end to end
encrypted; control sees metadata only (who, which host, when, sizes, names).
Every client-to-daemon stream through control is one Noise channel
(`Noise_IK_25519_AESGCM_SHA256`), on the relay and on direct paths from
control's page, with one channel per viewer so revoking closes it. A page a
daemon serves itself still uses plain `/ws`, inside the tailnet's WireGuard.
The exception is chat (M122, "Chat lives in control, in plain text"):
control reads a team's chat, and the lines someone chooses to quote from a
pane into it. A pane's output, input and scrollback stay end to end.
Where: `crates/e2e/src/channel.rs`, `crates/control/src/relay.rs`, `crates/daemon/src/e2e.rs`.
From: [Control track](docs/plan-archive.md#control-track-s15-m17m22-added-2026-10-01), [M18](docs/plan-archive.md#m18-relay-and-end-to-end-encryption), docs/control-e2e.md.
