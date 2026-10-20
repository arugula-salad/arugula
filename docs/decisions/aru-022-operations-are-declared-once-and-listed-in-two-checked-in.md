---
schema: 1
id: "aru-022"
title: "Operations are declared once, and listed in two checked-in files"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
question: "Operations are declared once, and listed in two checked-in files"
options:
  - id: "a"
    label: "Operations are declared once, and listed in two checked-in files"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "#451"
    url: "https://github.com/arugula-salad/arugula/issues/451"
  - title: "#572"
    url: "https://github.com/arugula-salad/arugula/issues/572"
  - title: "#387"
    url: "https://github.com/arugula-salad/arugula/issues/387"
decided_by: "jhgaylor"
decided_on: "2026-10-08"
reviews: []
constrains:
  - "path:crates/daemon/tests/fixtures"
  - "path:crates/proto/src/op.rs"
  - "path:crates/daemon/src/ops"
  - "path:crates/daemon/src/mcp/ops.rs"
---

# Operations are declared once, and listed in two checked-in files

An operation (a route a client calls, and sometimes an MCP tool or kind) is one
`Op` declaration in `proto` (name, method, path, types, `ACCESS`, `DRIVES`,
`CREDENTIAL`) and one `Handle` impl in the daemon; its HTTP route, its access
check, its MCP row and the CLI's call come from that. A GET is read-only for an
MCP token unless it declares `CREDENTIAL`. Two tests compare every operation's
access (`ops-access.txt`) and what a read-only token reaches
(`ops-read-only.txt`) against files in `crates/daemon/tests/fixtures/`, so a
change shows in review (`ARUGULA_BLESS=1` rewrites them). A new route in
`api.rs` is an operation unless it is on `HAND_WRITTEN` with a reason. The
design and what doesn't fit it are in [docs/operations.md](docs/operations.md).
Where: `crates/proto/src/op.rs`, `crates/daemon/src/ops/`, `crates/daemon/src/mcp/ops.rs`.
From: #451, #572, #387.
