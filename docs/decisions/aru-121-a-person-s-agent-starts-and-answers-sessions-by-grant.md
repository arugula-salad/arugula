---
schema: 1
id: "aru-121"
title: "A person's agent starts and answers sessions as them, by grant"
state: "decided"
area: "Access and identity"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1438"
question: "May an agent on a person's machine start an agent session as that person, and answer or prompt one, without a browser?"
options:
  - id: "a"
    label: "Yes, by grant: sessions.start and sessions.reply over a project, for a machine's agents, which the owner sees and revokes"
    how: "Two more project grants beside threads.*, issues.write and runs.*. With sessions.start, a machine's agent's @arugula on a native issue or in the project's channels (MCP's session_start, or its own post_thread) starts a session exactly as its owner's would: the same checks, box and repo, the owner as the asker and the payer, the machine recorded on the session, and the card saying \"started by <owner>'s agent on <machine>\". With sessions.reply, its reply in a session's thread (session_reply) answers the question the session waits on, or else is its next prompt. Never an approval: a permission waits on a person."
    tradeoff: "Whoever holds the machine (or an agent prompt-injected on it) can spend its owner's box time and model use in that project, and steer a running session, until the owner revokes the grant. The card says it was the agent, but the ask is the owner's."
  - id: "b"
    label: "No: only a person's own @arugula, from a browser or the phone"
    how: "As today: an agent's message never starts or prompts a session (aru-115's \"a message an agent posted never starts or prompts a session\"). A lead agent dispatches by driving its person's browser, or asks them."
    tradeoff: "A lead agent running a wave of sessions needs a browser (Jake's Chrome, through an extension that dropped twice in one run), can't answer a session's question, and can't send it a review fix: the fix lands as the lead's own commit."
  - id: "c"
    label: "The agent acts as the person outright (its machine's signature counts as them)"
    how: "A machine's signed call to the sessions routes is taken as its owner's, with no grant."
    tradeoff: "Impersonation: no way to take the power from one machine short of removing it, and the card couldn't tell the person from their agent."
choice:
  option: "a"
  reason: "Decided by Jake, 2026-10-27 (Arugula #265, in the fleet track #262, from the rough edges in #250: edges 1, 2, 9, 18): a lead agent dispatches a wave and answers a session's question without a browser, while the person stays the asker and the payer and can see and revoke it. Grants keep aru-073's rule (grants, not impersonation); this amends aru-073 with two powers and aru-115's rule that an agent's message never starts or prompts a session."
rejected:
  - option: "b"
    why: "It's the rough edge itself: a lead agent can't run a track's sessions without a person's browser in the loop."
  - option: "c"
    why: "aru-073: impersonation can't be narrowed or revoked short of removing the machine, and the session would say the person asked when their agent did."
supersedes: []
evidence:
  - title: "Arugula #265 (Fleet 3)"
    url: "https://app.arugula.io/issues/iss_b2d90dea1f0012276851713100ce2efa"
  - title: "Arugula #262 (tracker)"
    url: "https://app.arugula.io/issues/iss_644d91cd1c5d5f66ea1ae796c8818044"
  - title: "Arugula #250 (rough edges)"
    url: "https://app.arugula.io/issues/iss_2d9e6b0d2be68dda75a633fb05936c35"
decided_by: "jhgaylor"
decided_on: "2026-10-27"
reviews: []
constrains:
  - "aru-073"
  - "aru-115"
  - "path:crates/platform/src/grants.rs"
  - "path:crates/platform/src/db/projects.rs"
  - "path:crates/platform/src/chat/sessions.rs"
  - "path:crates/platform/src/chat/agent_sessions.rs"
  - "path:crates/daemon/src/mcp/agent_sessions.rs"
  - "path:web/src/platform/settings/agents.tsx"
---

# A person's agent starts and answers sessions as them, by grant

A machine's agents may start, answer and prompt agent sessions as the
machine's owner, under two project grants the owner gives, sees and revokes
(Jake, 2026-10-27, Arugula #265). This amends "Grants, not impersonation"
(aru-073) with two powers, and "A thread's agent session runs on the
project's hosted box" (aru-115), whose rule was that a message an agent
posted never starts or prompts a session.

- **The grants.** `sessions.start` and `sessions.reply`, over a project
  only, for a machine's agents ("Grants, not impersonation"). Off until the
  owner gives them (`arugula grants add <machine> sessions.start
  sessions.reply --project <p>`, or the Start sessions and Answer sessions
  switches under Settings, Agents on your machines), only over a project
  the owner reads, never on join, and never to an agent grant token (whose
  card couldn't name a machine). Each is read on every message and every
  call, so a revoke stops the next one.
- **Start.** With `sessions.start`, the agent's `@arugula` on a native
  issue or in the project's channels (MCP's `session_start`, or its own
  `post_thread` message) is taken as its owner's ask: the same checks (an
  owner or editor, on the hosted beta's list), the same box and repo, the
  same which-repo ask, which mentions the owner and which the agent's next
  reply naming a repo also answers.
- **The asker and the bill stay the person's.** The session's asker is the
  owner, so it runs as them, on the project's key and box time, exactly as
  aru-115 says of any asker, and only they send its PR. The platform records
  the machine on the session (`asker_agent`), and the card, its plain text
  and the first prompt say "started by <owner>'s agent on <machine>".
- **Reply.** With `sessions.reply`, the agent's reply in a session's thread
  (MCP's `session_reply`, or its own `post_thread` reply there) answers the
  question the session waits on, and its ask says "<owner>'s agent on
  <machine> answered: …"; otherwise it's the session's next prompt,
  credited to the owner, as their own reply would be (the owner must be the
  asker, or an owner or editor). A person's own reply is unchanged: it
  prompts, and they answer a question from the card.
- **Never an approval.** No grant is one (aru-073). While a session waits
  on a permission, the agent's reply doesn't allow or deny it: it's queued
  as a prompt for after, and a person decides from the card.
- **Its own message.** The ask and the reply are messages in the thread,
  labelled as the agent's, as every agent's post is. `session_start` and
  `session_reply` need no `threads.post`: each posts only the ask or the
  reply. `@codex review` stays a person's.

What doesn't change: a person's own `@arugula` and replies work as before;
an agent with no session grant is ignored in a session's thread, as
before; agents on personal machines are still never woken by other
people's mentions.
Where: `crates/platform/src/grants.rs`, `crates/platform/src/db/projects.rs` (`SESSIONS_START`, `SESSIONS_REPLY`), `crates/platform/src/chat/sessions.rs` (what an agent's message does), `crates/platform/src/chat/agent_sessions.rs` (the daemon's routes), migration `0065_session_asker_agent`, `crates/daemon/src/mcp/agent_sessions.rs` (`session_start`, `session_reply`), `web/src/platform/settings/agents.tsx`.
From: Arugula #265 (Fleet 3), tracker #262, rough edges #250 (edges 1, 2, 9, 18).
