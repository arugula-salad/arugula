---
schema: 1
id: "aru-133"
title: "A thread shows its agents, and control keeps none of their live state"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  harness: "claude-code"
question: "When agents take part in a thread, what does a person see of them, and where does that come from, given that terminal content stays out of control (aru-051)?"
options:
  - id: "a"
    label: "A roster from what control already has, with live state asked of each machine on read"
    how: "Agents' posts and the thread's mention statuses say who's in it and how far each has read. Each read asks the machine's daemon (`agents.sessions` with `only`) what those sessions are doing. Control stores none of that answer."
    tradeoff: "A read costs a call to each machine and can take up to the 5 s a daemon has to answer. An offline machine says nothing but that it's offline."
  - id: "b"
    label: "Daemons push state to control, which stores it per session"
    how: "A report on each turn and tool call, kept in a table and sent live to the page."
    tradeoff: "Instant and cheap to read, but control then holds a running record of what every session is doing, on every machine, whether anyone is looking or not."
  - id: "c"
    label: "A new agent thread object"
    how: "A thread kind of its own, with its own participants and lifecycle."
    tradeoff: "Clean to model, but it's a second kind of conversation, and existing threads and session cards wouldn't gain anything."
choice:
  option: "a"
  reason: "It adds no new object, and it keeps the aru-051 line: control only passes along what the owner's `agent_sessions` setting shows, at the moment someone is looking. A state word shows at `names`, the default. The tool's name and the work item show only at `work`. Output, tool arguments, paths and summaries written by the agent never cross."
rejected:
  - option: "b"
    why: "It would store activity nobody asked to see. If polling costs too much, a push can come later without changing what the page reads."
  - option: "c"
    why: "Fable and Codex, asked separately, each said the same: this grows threads and the session card. The hosted session card is already this thing for one agent."
supersedes: []
evidence:
  - title: "Arugula native #645 (this record)"
    url: "https://app.arugula.io/issues/iss_ed8701128f4da177d36e7126038a6558"
  - title: "Agent-native threads board"
    url: "https://app.arugula.io/p/prj_3e5d802cb4b08d43074df6c32f5344c1/boards/brd_7c01987487e7fb44de9104f0171bc60a"
decided_by: "jhgaylor"
decided_on: "2026-11-03"
reviews: []
constrains:
  - "aru-051"
  - "aru-091"
  - "aru-115"
  - "path:crates/platform-chat/src/chat/thread_agents.rs"
  - "path:crates/daemon/src/agent_mentions.rs"
---

Personal agents are woken by other people's mentions today (#394, #575), so the "never woken" in aru-091 and aru-115 is out of date. This record doesn't change who can wake an agent: its owner's `agent_mentions` switch still decides that, and an agent's post wakes another agent only by name (#642).
