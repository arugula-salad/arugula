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
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/daemon/src/forge"
---

# Forge blocks use the person's own CLI, and agents draft while people send

One `forge` block type with a provider adapter (GitHub through `gh`, Forgejo
through `tea`, GitLab through `glab`); illogical stores no forge tokens. An
agent's write (comment, approve, merge, close) becomes a draft on the block;
the owner or an editor sends it, as themselves. Freshness is conditional
polling, with webhooks relayed by control's GitHub App. Launch ships GitHub PR
blocks only; the other providers move into Labs (#457, not yet built).
Where: `crates/daemon/src/forge/`.
From: [Forge track](docs/plan-archive.md#forge-track-s23-m36m40-added-2026-10-02), [M36](docs/plan-archive.md#m36-forgejo-pull-request-blocks-88), #457.
