---
schema: 1
id: "aru-134"
title: "One data model: what each noun means and where its truth lives"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  harness: "claude-code"
question: "The same words cover unrelated things across the machine, the hub and the platform, and some data has two homes. What does each noun mean, and which home is the truth?"
options:
  - id: "a"
    label: "Keep today's words where they're public, and give each meaning its own name"
    how: "Session stays the group of tabs, a Cloud workspace holds a hosted machine, a machine's Doc is the truth over its platform copy, and a team keeps its one default project. Written down in docs/data-model.md."
    tradeoff: "Session stays a slightly odd word for an agent product, and teams can't separate two products yet."
  - id: "b"
    label: "Rename toward the platform and move truth there"
    how: "Session means an agent, the group of tabs gets a new name, a Cloud workspace is the only noun for a hosted machine, the platform owns Docs, and teams can make more projects."
    tradeoff: "Reads better for teams, but changes the UI, CLI flags and the unit of sharing just before launch, and Docs would need an offline story."
choice:
  option: "a"
  reason: "It needs the fewest public changes before launch, matches what the code and its signed certificates already say, and stops the one way data is silently lost, without closing off option b for Docs or projects later (Jake, 2026-11-03)."
rejected:
  - option: "b"
    why: "Too much change for a launch two weeks away. Platform-owned Docs and more projects can each come later when someone needs them."
supersedes: []
evidence:
  - title: "Arugula native #666 (this record)"
    url: "https://app.arugula.io/issues/iss_fe89bca358e49671e08fa3849533e978"
decided_by: "jhgaylor"
decided_on: "2026-11-03"
reviews: []
constrains:
  - "aru-072"
  - "aru-074"
  - "aru-118"
  - "path:docs/data-model.md"
---

# One data model: what each noun means and where its truth lives

The product went from machine-first to teams to a project platform quickly, and
each shift added nouns without retiring the old ones. Five decisions settle the
worst of it (Jake, 2026-11-03), and [docs/data-model.md](../data-model.md) holds
the whole model, including where the code doesn't match it yet.

- **A hosted machine runs inside a Cloud workspace.** The workspace (`hw_`) is
  the project's durable object. The hosted machine is what currently runs in it,
  with its own keys, and a rebuild replaces it. People see "Cloud workspace",
  and "box" isn't used for this.
- **"Session" is the group of tabs.** An "agent session" is only ever the
  platform's `ses_`. A live agent in a pane is an "agent". Claude Code's session
  is a "conversation". This amends aru-074.
- **A machine's Doc is the truth, and its platform copy is a copy.** The copy
  becomes read-only and says so, which stops web edits being silently overwritten
  by the machine's next change. This amends aru-118. Moving truth to the platform
  stays open.
- **A team has its one default project for now.** aru-072 already gives a team a
  default project. Nothing creates a second until a team needs one.
- **A room is any place people chat.** "Thread" is only for replies and threads
  on an object, and "conversation" only for Claude Code. MCP's `read_thread` and
  `post_thread` keep their names for launch.

The security distinctions that look like duplicates stay as they are:
`docs/data-model.md` lists them under "Don't merge these".

Where: [docs/data-model.md](../data-model.md).
From: Arugula native #666.
