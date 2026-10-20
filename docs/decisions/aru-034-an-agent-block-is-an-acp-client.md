---
schema: 1
id: "aru-034"
title: "An agent block is an ACP client"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "An agent block is an ACP client"
options:
  - id: "a"
    label: "An agent block is an ACP client"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M6b"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m6b-agent-blocks-acp-clients"
  - title: "#166"
    url: "https://github.com/arugula-salad/arugula/issues/166"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/daemon/src/agent/mod.rs"
  - "path:crates/daemon/src/rules.rs"
---

# An agent block is an ACP client

It speaks the Agent Client Protocol (JSON-RPC on the agent's stdio) to
whatever agent server the block names (Claude Code through
`claude-agent-acp`, Codex, any ACP command). One protocol for every
agent, so no per-agent adapter. A permission request puts the block in
`needs-input`; an unanswered question or approval waits indefinitely. "Always"
rules are kept by the daemon in `rules.json`, per machine, never in Claude
Code's settings.
Where: `crates/daemon/src/agent/mod.rs`, `crates/daemon/src/rules.rs`.
From: [M6b](docs/plan-archive.md#m6b-agent-blocks-acp-clients), S7, #166.
