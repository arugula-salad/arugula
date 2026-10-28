---
schema: 1
id: "aru-045"
title: "A recipe is a Claude Code subagent file, worn on this host"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "A recipe is a Claude Code subagent file, worn on this host"
options:
  - id: "a"
    label: "A recipe is a Claude Code subagent file, worn on this host"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "S34"
    url: "https://github.com/arugula-salad/arugula/issues/397"
  - title: "#398"
    url: "https://github.com/arugula-salad/arugula/issues/398"
  - title: "#403"
    url: "https://github.com/arugula-salad/arugula/issues/403"
decided_by: "jhgaylor"
decided_on: "2026-10-15"
reviews: []
constrains:
  - "path:crates/arugulad-labs/src/agents"
  - "path:crates/arugulad-labs/src/wearing.rs"
  - "path:crates/cli/src/cmd/agents.rs"
---

# A recipe is a Claude Code subagent file, worn on this host

The agents a person offers to other people's agents (#403) are
`.claude/agents/NAME.md` files, read as they are and never written, so the
same file is a subagent in the person's own Claude Code. Claude Code's ACP
adapter drops the SDK's `agent` option, so a block can't select one: it wears
it instead, through `Worn` (M44's, from when a Fountain agent could be worn
here too; that was retired on 2026-10-15, and the shared code moved to
`labs/wearing.rs`). The prompt is appended after a line naming
the agent, and model, `tools`, `disallowedTools` and `permissionMode` are
applied (a block's own `permission_mode` wins). Skills are preloaded into the
prompt, as Claude Code does for a subagent: a recipe's `tools` can leave out
the Skill tool, and a plugin alone was never read in a real run. Named MCP
servers come from the scopes Claude Code reads (local, `.mcp.json`, user),
through Fountain's by-reference server code. The session gets
`--strict-mcp-config`, because an agent other people will task mustn't
carry the account's claude.ai connectors (Gmail and the like), and a real
run showed those come along otherwise. Offering is per machine and the
owner's (`<state>/agent-offers.json`). A card is A2A 1.0 with an
`arugula://` JSON-RPC interface and a required channel extension. `agent
--as` names a recipe (Fountain's `--as-fountain`, which it once was, went
with M44).
Where: `crates/arugulad-labs/src/agents/`, `crates/arugulad-labs/src/wearing.rs` (`Worn`), `crates/cli/src/cmd/agents.rs`.
From: [S34](https://github.com/arugula-salad/arugula/issues/397), M76 (#398), #403.
