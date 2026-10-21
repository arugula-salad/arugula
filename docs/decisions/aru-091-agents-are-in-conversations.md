---
schema: 1
id: "aru-091"
title: "Agents are in conversations"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1264"
question: "May agents read and write team channels, DMs, object threads and native issues, and what bounds them?"
options:
  - id: "a"
    label: "Yes, through grants on the machine: off by default, scoped, labelled, disclosed and revocable"
    how: "Three powers in aru-073's grants table: threads.read (read rooms and issues in scope), threads.post (post messages, issue comments included) and issues.write (create, edit, close, label and assign native issues). The subject is the machine (subject_kind daemon); the scope is a project (a team's channels are its default project's, object threads and issues their project's) or the owner's account (their DMs). Only the machine's owner grants, only over scopes they can read. What an agent writes keeps the owner as author plus the machine, shown as \"<owner>'s agent on <machine>\". Every room a grant reaches says whose agents can read it. An @ naming a machine's agents is a mention readable over MCP; nothing wakes an agent. MCP's thread and issue tools call the platform, signed by the machine; the daemon keeps no thread store."
    tradeoff: "People's rooms hold text agents wrote, and a grant reaches every agent on the machine, not one."
  - id: "b"
    label: "Keep agents out of chat"
    how: "As aru-052 had it since M121: no read_thread or post_thread, and an @agent reaches no one."
    tradeoff: "Agents can't work on native issues or answer in the threads about the work."
  - id: "c"
    label: "Bring back the daemon's thread store, with links and leads"
    how: "As M81-M85 had it: threads on the machine, conversations with linked panes and a lead agent, and conv-grants.json."
    tradeoff: "Threads vanish while their machine sleeps, and conversation grants were a second, local permission system."
  - id: "d"
    label: "The thread powers on by default"
    how: "Every joined machine gets them over its owner's scopes, backfilled as cloud.drive was."
    tradeoff: "Agents appear in every room their owner can read, teammates' channels and DMs included, without anyone choosing it."
choice:
  option: "a"
  reason: "Agents everywhere (Jake, 2026-10-21, #1263): agents read and post where the work is discussed, never reaching more than their owner, always labelled and disclosed, and revocable on the next call. Off by default, granted per scope, is the lead's call in #1263."
rejected:
  - option: "b"
    why: "Superseded by Jake: chat is control's and grants exist, which is what aru-052 said agents waited on (#959, #1002)."
  - option: "c"
    why: "M125 took it out: the platform keeps threads while machines sleep, and grants are the one permission system."
  - option: "d"
    why: "Rooms would gain agents without anyone in them choosing it; the owner turns each scope on."
supersedes:
  - decision: "aru-052"
evidence:
  - title: "#1263"
    url: "https://github.com/arugula-salad/arugula/issues/1263"
  - title: "#1264"
    url: "https://github.com/arugula-salad/arugula/issues/1264"
  - title: "#1268"
    url: "https://github.com/arugula-salad/arugula/issues/1268"
  - title: "#1269"
    url: "https://github.com/arugula-salad/arugula/issues/1269"
  - title: "#1271"
    url: "https://github.com/arugula-salad/arugula/issues/1271"
  - title: "#1273"
    url: "https://github.com/arugula-salad/arugula/issues/1273"
  - title: "#1095"
    url: "https://github.com/arugula-salad/arugula/issues/1095"
  - title: "#959"
    url: "https://github.com/arugula-salad/arugula/issues/959"
  - title: "#1002"
    url: "https://github.com/arugula-salad/arugula/issues/1002"
decided_by: "jhgaylor"
decided_on: "2026-10-21"
reviews: []
constrains:
  - "path:crates/platform/src/grants.rs"
  - "path:crates/platform/src/auth.rs"
  - "path:crates/platform/src/chat.rs"
  - "path:crates/daemon/src/mcp/tools.rs"
---

# Agents are in conversations

Agents are where people talk about the work (Jake, 2026-10-21, #1263:
"agents everywhere"). This supersedes "Agents are out of chat". Agents read
and post in team channels, DMs, object threads and native issues, through
grants ("Grants, not impersonation").

- **The subject is the machine.** The daemon signs every MCP call as itself,
  so a grant names a machine, not one agent on it. An agent never reaches
  more than its machine's owner can.
- **Three powers,** as rows in the `grants` table with subject kind `daemon`
  and the machine's id:
  - `threads.read` reads rooms and issues in scope;
  - `threads.post` posts messages, issue comments included;
  - `issues.write` creates, edits, closes, labels and assigns native issues.
- **A grant's scope is a project or the owner's account.** A team's channels
  are its default project's, and object threads and issues are their
  project's ("The project is the unit of platform sharing"). The owner's DMs
  are account-scoped.
- **Off by default, granted per scope** (the lead's call in #1263, which Jake
  may overturn). Only the machine's owner grants, and only over scopes they
  can read, with one command (`arugula grants add`) or one switch on the
  platform page. A call without a grant is refused with a message saying how
  to grant one. Revoking takes effect on the next call; posts already made
  stay, still labelled.
- **Labelled.** What an agent writes keeps its owner's account as author,
  plus the machine (`agent` on `chat_messages`, `author_agent` on `issues`
  and `issue_events`). It's shown everywhere as "`<owner>`'s agent on
  `<machine>`", in its own style.
- **Disclosed.** Every room an agent grant reaches says so, naming whose
  agents, to everyone in it, DMs included.
- **Mentions are read, not pushed.** An `@` naming a machine or its agents is
  stored as a mention that `read_thread` returns over MCP. Nothing wakes or
  dispatches an agent in this step.

What stays removed (M121, M125, #1095): the daemon keeps no thread store, and
MCP's thread and issue tools call the platform, signed by the machine (the
platform's `Machine` extractor). Conversations don't come back as they were:
no links to panes, no leads, no events or cards in a timeline, and no
`conv-grants.json`. Messages an agent posted before M121 still read as the
text they were kept with.

Writes to GitHub stay a person's: an agent's GitHub write is a draft its
owner sends ("Forge blocks use the person's own CLI, and agents draft while
people send", amended). Chat now stores text agents wrote ("Chat lives in
control, in plain text; terminal content doesn't", amended), and the powers
join `cloud.drive` in "Grants, not impersonation" (amended).
Where: `crates/platform/src/grants.rs`, `crates/platform/src/auth.rs` (`Machine`), `crates/platform/src/chat.rs`, `crates/daemon/src/mcp/tools.rs` (M198, M200).
From: #1263 (platform roadmap, step 6), M193 (#1264), M197 (#1268), M198 (#1269), M200 (#1271), M202 (#1273), #959 and #1002 (agent grants in teams), M121 and M125 (#1095).
