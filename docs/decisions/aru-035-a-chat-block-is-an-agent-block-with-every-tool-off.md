---
schema: 1
id: "aru-035"
title: "A chat block is an agent block with every tool off"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "A chat block is an agent block with every tool off"
options:
  - id: "a"
    label: "A chat block is an agent block with every tool off"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence: []
decided_by: "jhgaylor"
decided_on: "2026-10-19"
reviews: []
constrains:
  - "path:crates/daemon/src/agent/defs.rs"
  - "path:crates/daemon/src/agent/mod.rs"
  - "path:web/src/blocks/chat.tsx"
---

# A chat block is an agent block with every tool off

A chat (a plain conversation with a model, #1102, 2026-10-19) is the
agent block's ACP code under a type of its own, not a model client of
Arugula's: Claude Code on the machine's login, so no API keys. Its session
gets `tools: []`, no settings sources, `--strict-mcp-config` and no MCP
servers (Arugula's neither), and a system prompt of its own, in a folder of
the block's in the state directory; its config keeps nothing that gives an
agent tools or a project, and a permission request is turned down unseen.
It's per machine, kept and restored by that daemon like an agent block.
Codex isn't one: codex-acp 2.1.0 has no switch for every tool (the shell
goes, apply_patch, `exec` and subagents stay).
Where: `crates/daemon/src/agent/defs.rs` (`chat_meta`), `crates/daemon/src/agent/mod.rs` (`ChatKind`), `web/src/blocks/chat.tsx`.
