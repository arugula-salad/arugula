---
schema: 1
id: "aru-067"
title: "Huddles are removed"
state: "decided"
area: "Build and release"
source:
  via: "cli"
question: "Huddles are removed"
options:
  - id: "a"
    label: "Huddles are removed"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence: []
decided_by: "jhgaylor"
decided_on: "2026-10-18"
reviews: []
constrains:
  - "path:scripts/macos-sign"
  - "path:crates/proto/src/flags.rs"
  - "path:crates/daemon/tests/integration/flag_gates.rs"
  - "path:crates/desktop/Info.plist"
---

# Huddles are removed

Huddles (M63: voice calls on a session, peer to peer over WebRTC, signalled
through the daemon, with TURN credentials from control and a native call
peer in the Linux app) were taken out of Arugula entirely on 2026-10-18,
Jake's decision. Revive them from git if they're wanted again: everything
is in the tree at f9c8217b (`crates/mux/src/labs/calls.rs`,
`crates/mux/src/mux/call_ops.rs`, `crates/desktop/src/calls.rs`,
`crates/control/src/turn.rs`, `web/src/call.ts`, `web/src/ui/huddle.tsx`
and their tests). What stays, for a fleet whose versions are mixed:
- **`huddles` is a retired flag** (`flags::RETIRED`, "A retired flag is a
  name that isn't one", above):
  a `flags.json` or a control push that names it loads, means nothing, and
  is kept by a write; setting it says why it's gone.
- **An older page's call messages are dropped.** `ClientMsg` has no
  `call_*` variants now, so one fails to parse and the daemon logs it as a
  bad client message and carries on, as it does for any message it doesn't
  know. An older daemon's `calls` (in `State`, `Delta` and
  `HostFeatures`) and `call_signal` are ignored by a newer page and TUI (since removed),
  which never ask for them. `GET /api/turn` and control's
  `GET /api/daemon/turn` are gone; an older daemon asking control falls
  back to public STUN, as it did when control couldn't be reached.
- **Telemetry's `huddle` view and `huddles` flag** were dropped from the
  closed sets; an older daemon's report counts them as `other`.
- **The macOS app keeps its microphone** (`NSMicrophoneUsageDescription`,
  the `audio-input` entitlement and `scripts/macos-sign`'s check): a lent
  device's `record_audio` (S33) records in the app's webview too.
Where: `crates/proto/src/flags.rs` (`RETIRED`), `crates/daemon/tests/integration/flag_gates.rs` (`a_call_message_from_an_older_page_is_dropped`), `crates/desktop/Info.plist`.
From: Jake, 2026-10-18.
