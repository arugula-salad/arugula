---
schema: 1
id: "aru-052"
title: "Agents are out of chat"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "Agents are out of chat"
options:
  - id: "a"
    label: "Agents are out of chat"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "S41"
    url: "https://github.com/arugula-salad/arugula/blob/main/spikes/s41-chat-in-control/README.md"
  - title: "#1095"
    url: "https://github.com/arugula-salad/arugula/issues/1095"
decided_by: "jhgaylor"
decided_on: "2026-10-19"
reviews: []
constrains:
  - "path:crates/daemon/src/mcp/tools.rs"
  - "path:crates/platform/src/chat.rs"
---

# Agents are out of chat

Superseded by "Agents are in conversations" (aru-091; Jake, 2026-10-21,
#1263: "agents everywhere"). What follows is the decision as it stood from
M121 until then.

Threads and conversations are people's (Jake, 2026-10-19, S41 decisions 8
and 9). No agent reads or posts in them: MCP has no `read_thread` or
`post_thread` (and since M125 the daemon has no thread operations). `@agent`,
`@claude` and `@lead` are text like any other `@`, and reach no one. A
conversation has no links to panes, no grants on them, no events or cards
in its timeline and no lead (M82–M85 are removed, with `conv-grants.json`
and control's `AccessList.links` and `reasons`); a member added from outside
the owner's teams gets control's plain share offer. Old messages an agent
posted, or a link's events, read as the text they were kept with. Agents
come back into chat only through the grants #959 and #1002 work out, once
chat is control's (#978), not through the daemon's thread store.
Where: `crates/daemon/src/mcp/tools.rs` (the tool list), control's `crates/platform/src/chat.rs`.
From: [S41](spikes/s41-chat-in-control/README.md) q7 and decisions 8 and 9, M121 (#1095).
