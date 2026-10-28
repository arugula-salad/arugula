---
schema: 1
id: "aru-041"
title: "Forge blocks use the person's own CLI, and agents draft while people send"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "Forge blocks use the person's own CLI, and agents draft while people send"
options:
  - id: "a"
    label: "Forge blocks use the person's own CLI, and agents draft while people send"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Forge track"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#forge-track-s23-m36m40-added-2026-10-02"
  - title: "M36"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m36-forgejo-pull-request-blocks-88"
  - title: "#457"
    url: "https://github.com/arugula-salad/arugula/issues/457"
  - title: "#1204"
    url: "https://github.com/arugula-salad/arugula/issues/1204"
  - title: "#1205"
    url: "https://github.com/arugula-salad/arugula/issues/1205"
  - title: "#1214"
    url: "https://github.com/arugula-salad/arugula/issues/1214"
  - title: "#1263"
    url: "https://github.com/arugula-salad/arugula/issues/1263"
  - title: "#1264"
    url: "https://github.com/arugula-salad/arugula/issues/1264"
  - title: "#1271"
    url: "https://github.com/arugula-salad/arugula/issues/1271"
  - title: "#1276"
    url: "https://github.com/arugula-salad/arugula/issues/1276"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/arugulad-forge/src"
---

# Forge blocks use the person's own CLI, and agents draft while people send

One `forge` block type with a provider adapter (GitHub through `gh`, Forgejo
through `tea`, GitLab through `glab`); illogical stores no forge tokens. An
agent's write (comment, approve, merge, close) becomes a draft on the block;
the owner or an editor sends it, as themselves. Freshness is conditional
polling, with webhooks relayed by control's GitHub App. Launch ships GitHub PR
blocks only; the other providers move into Labs (#457, not yet built).

Since step 4 (#1204) the platform can send too. For a repo linked to a
project, a person's send goes through the platform with a one-use GitHub App
user token, dropped after the one call, or through one of their online
machines with `gh` ("Sending as a person uses a one-use user token, falling
back to their machine"). The platform stores no forge tokens either. Drafts
on linked repos are platform rows, and blocks on them are pins ("The
daemon's forge reader stays for unlinked repos; linked repos' blocks are
pins"). "Agents draft, people send" still holds: a send is always a person's
action, and no grant makes an agent a sender.

**Native issues aren't forge objects** (step 6, #1263). An agent whose
machine has an `issues.write` grant writes a project's native issues
directly: it creates, edits, labels, assigns and closes them, and with
`threads.post` comments on them, labelled as its owner's agent ("A project
owns numbered issues", "Agents are in conversations"). GitHub writes don't
change: an agent's comment, review or issue on GitHub is still a draft a
person sends through "Sending as a person uses a one-use user token, falling
back to their machine". So is an agent's native comment on an issue exported
to GitHub, which stays native until it's sent ("Taking custody of issues
stays reversible"). MCP's tool descriptions say which is which, so an agent
knows when a person must send.

**A session's box may push its own branch** (thread = session, Arugula
#129). An agent session on a project's hosted box pushes
`arugula/<session>-<slug>`, and only that ref, with an installation token
minted for each push and never written to disk. That's the one push a
machine makes. The PR stays a draft the session's asker sends, with their
one-use user token: machines never send, merge or approve ("A thread's agent
session runs on the project's hosted box").
Where: `crates/arugulad-forge/src/`.
From: [Forge track](docs/plan-archive.md#forge-track-s23-m36m40-added-2026-10-02), [M36](docs/plan-archive.md#m36-forgejo-pull-request-blocks-88), #457, #1204 (step 4), M159 (#1205), M168 (#1214), #1263 (step 6), M193 (#1264), M200 (#1271), M205 (#1276).
