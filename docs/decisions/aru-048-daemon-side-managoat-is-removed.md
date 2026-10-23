---
schema: 1
id: "aru-048"
title: "Daemon-side Managoat is removed"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "Daemon-side Managoat is removed"
options:
  - id: "a"
    label: "Daemon-side Managoat is removed"
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
  - "path:crates/platform/src/cloud"
  - "path:crates/daemon/src/agent/defs.rs"
  - "path:crates/daemon/src/agent/mod.rs"
  - "path:crates/daemon/src/review/mod.rs"
  - "path:crates/daemon/src/mcp/tools.rs"
  - "path:crates/daemon/src/labs/agents/mod.rs"
  - "path:crates/proto/src/lib.rs"
  - "path:crates/daemon/tests/integration/agents.rs"
  - "path:crates/daemon/tests/integration/flag_gates.rs"
---

# Daemon-side Managoat is removed

The daemon's own Managoat integration (#942's F1 to F5: its device sign-in
and account stream, the account view block with its catalog, conversations
and permission-request cards, a conversation as an agent block on a native
link, recipes run on Managoat (from the Agents page, the agents block,
`agents run --managoat` and MCP's `start_agent {on: managoat}`), views of a
conversation's sandbox, `arugula managoat`, MCP's Managoat kinds, and the
per-machine default for Managoat approvals) was retired by "Cloud execution
is owned by Arugula control" (above) and taken out on 2026-10-18, Jake's
decision. Control's own Managoat adapter (`crates/platform/src/cloud/`) was
separate and stayed, until it went too (2026-10-23, M189 and M190). This ends "Fountain is a headless backend, and Arugula
is its UI" (#942, 2026-10-15), and with it the entries on the daemon's
sign-in, the account view's cards, recipes on Managoat and the rename's
aliases; S34's "no Fountain dependency" holds again for the daemon. Revive
any of it from git: everything is in the tree at 50daf913
(`crates/daemon/src/labs/fountain/`, `crates/cli/src/cmd/fountain.rs`,
`web/src/blocks/fountain.tsx`, `web/src/ui/fountain-run.tsx` and their
tests). What stays, for saved state and a fleet whose versions are mixed:
- **A saved Managoat agent block is an archived transcript.** `agent:
  managoat` (`fountain` before the rename) and `managoat_agent` still
  parse; the block comes back with its transcript, stopped, saying where
  cloud agents went, and refuses every call but reading it and its title.
  A new one is refused with the same words.
- **A saved account view, or a diff or file view of a sandbox, is
  dropped.** `BlockType` has no `managoat` now, so one parses as `Unknown`,
  which no kind makes, and restoring drops it, as it does any block it
  can't make; a view with `fountain` in its config is refused, and so
  dropped too. They held nothing of their own: what they showed lives on
  Managoat. `WorkKind`'s `managoat` stays, so an older daemon's panes
  still parse.
- **Old names are refused, not run as something else.** MCP's Managoat
  kinds (by either name) and `start_agent` with `agent: managoat`,
  `managoat_agent` or `on: managoat` say where cloud agents went; `POST
  /api/a2a/run` with an older CLI's `fountain: true` is refused (410), not
  run on another machine. `arugulad --managoat-account-stream` still
  parses, so an old service file starts, and logs that it does nothing.
- **`fountain` (and `managoat`) stay retired flags** (`flags::RETIRED`,
  "A retired flag is a name that isn't one" under Build and release).
- **An older daemon's `features.fountain`** is ignored: no page or CLI
  reads it. `/api/fountain/…`, `/api/agent-approvals` and `/api/a2a/
  fountain/…` are gone; `agent-approvals.json` and `managoat/` in a state
  dir are left as they are.
Where: `crates/daemon/src/agent/defs.rs` (`Kind::Managoat`, `CLOUD_MIGRATION`), `crates/daemon/src/agent/mod.rs` (`begin`), `crates/daemon/src/review/mod.rs` (`SANDBOX_GONE`), `crates/daemon/src/mcp/tools.rs` (`managoat_kind`, `managoat_start`), `crates/daemon/src/labs/agents/mod.rs` (`run_anywhere`), `crates/proto/src/lib.rs` (`BlockType`, `WorkKind`), `crates/daemon/tests/integration/agents.rs` (`legacy_cloud_transcript_restores_without_launching_or_accepting_commands`), `crates/daemon/tests/integration/flag_gates.rs`.
From: Jake, 2026-10-18.
