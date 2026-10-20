---
schema: 1
id: "aru-043"
title: "A workspace block asks chant, and keeps a few reads of its own"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "A workspace block asks chant, and keeps a few reads of its own"
options:
  - id: "a"
    label: "A workspace block asks chant, and keeps a few reads of its own"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "M34"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#m34-chant-workspace-blocks-73"
  - title: "#303"
    url: "https://github.com/arugula-salad/arugula/issues/303"
  - title: "#305"
    url: "https://github.com/arugula-salad/arugula/issues/305"
  - title: "#617"
    url: "https://github.com/arugula-salad/arugula/issues/617"
  - title: "#618"
    url: "https://github.com/arugula-salad/arugula/issues/618"
  - title: "#619"
    url: "https://github.com/arugula-salad/arugula/issues/619"
decided_by: "jhgaylor"
decided_on: "2026-10-07"
reviews: []
constrains:
  - "path:crates/daemon/src/labs/workspace/model.rs"
---

# A workspace block asks chant, and keeps a few reads of its own

A workspace block reads through the workspace's own chant and its contract-1
documents; chant decides what the workspace is and who reads it. An audit
against chant's workspace spec (INTENTIUS/chant 74b1a5b97, #305) found
smaller deviations. One was fixed: the fingerprint runs git with
`GIT_OPTIONAL_LOCKS=0` and `diff-index`, so polling never takes `index.lock`,
and its checksum covers `refs/chant/*` and `refs/heads/chant/work/*`. The
rest stay, each with a comment at the spot:
- Which chant (ws-021). The reader only finds a chant to start (`$CHANT`,
  `node_modules/.bin` here or above, PATH); chant hands off to the root's
  pinned one or refuses (`root-chant-required`, `reader-too-old`), and the
  block shows the code.
- Studio-box gates come from hud's work board (`apps/hud.rs`): the workspace
  is inside the box, where Arugula can't run chant. The board's gate fields
  are `status`'s.
- hud passes the email's local part as `onBehalfOf` (ws-080), since hud
  takes only a display name; a workspace with `identity.attribution:
  identified` refuses it in chant. The block's own approvals pass principals.
- Standing permission rules stay outside `chant audit --agents`
  (`rules.rs`): they are Arugula's, answering every ACP agent, and writing
  them into a harness's settings would allow those tools outside Arugula.
- Agent questions stay in Arugula, not chant `points` (ws-091): one person
  answering a hook can't meet a points record's rules.
- A workspace's decision points are answered on the swarm's rail, as its
  gates are approved (#621): a question `points --open` lists is a `Gate`
  with a `point` source, answered by `chant workspace points answer` with
  the block's principals and `--relayed-by`. An op gate that asks a point
  (chant#3170) is that question, never a `chant approve`. This is the
  approvals inbox behold#472 asked for, so behold doesn't build one.
- A member's agent sessions are `ls --json`'s `agents` for the member
  (#590), from the `ls` the block already runs. A chant that lacks the
  field, any before INTENTIUS/chant#3628 (no release has it yet), gets the
  reader's own parse of the declaration's file instead, `.json` or `.jsonc`
  (its comments and trailing commas taken out), read in the working tree
  where chant reads it at base. Drop that parse once the block's chant
  floor has the field.
- *Changes* on a member (#619) joins what chant can't know, and nothing
  else: a run in `graph --intent`'s `why` is linked to the open agent block
  whose recent turns wrote that run id, and a lease's holder to the open
  agent block of the workspace running as that session. The lease itself
  is `status`'s, matched to the walk's `work` nodes by record id, as the
  read contract joins documents; a lease on no work item the walk names
  isn't shown.
- `dist/fountain.yaml` and `src/agents` are read by hand: agent-specs is a
  chant project, not a workspace, and asking chant would mean a build.
- A forge issue keeps its own branch, not a chant work lease: the issue isn't
  a chant record.

The decisions a gate enforces come from `graph --intent <member dir>`
(#617), so chant does the covering and the block joins nothing (ws-052).
It's read per member with a gate waiting, outside the full read, and kept
until `HEAD` or the lifecycle ref moves (once per gate while nobody looks): it runs `git log` over the member, about a
second on a small repository and 40 s or more on chant's own, where the 30 s
limit on a host command cuts it off and the card says so. A member card
opened (#618) gets the same read, and the run ledger, only while a client
draws the block.

A member card joins a few things itself, accepted beside the pane mapping
because chant has no document that does them (#618):
- A run's pane: the run id Arugula writes (`arugula-<pane>-<ms>`), named
  only when the agent block in that pane recorded that run. chant knows the
  agent session; only Arugula knows the pane.
- A run on a member: its `agent` is a session the declaration binds to the
  member (`agents`), or chant's intent walk joined the run to the member's
  commits.
- A lease on a member: `status`'s lease is in the member's ledger, or its
  item is a work node in the member's intent graph with its own
  `constrains` (chant's statement that it covers the member).
- A lease's pane: the run whose `lease` is the lease's token, and that run's
  pane. A lease is never linked by its holder's name.
Where: `crates/daemon/src/labs/workspace/model.rs` (`READER`, `FINGERPRINT`, `INTENT`, `member_why`) and `why.rs`, `apps/hud.rs`, `rules.rs`, `cli/src/ask.rs`, `wearing.rs`, `forge/issue.rs`.
From: [M34](docs/plan-archive.md#m34-chant-workspace-blocks-73), #303, #305, #617, #618, #619.
