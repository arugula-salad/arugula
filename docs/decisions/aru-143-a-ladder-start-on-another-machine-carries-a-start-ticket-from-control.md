---
schema: 1
id: "aru-143"
title: "A ladder start on another machine carries a start ticket from control"
state: "decided"
area: "Agents and blocks"
source:
  via: "cli"
question: "A chief of staff or a board's lead starts an agent on another of its person's machines, for a purpose (a board to lead, an issue to work) and under a board's standing grant (#1416: machines, a cap, a lapse). Where is that grant checked: by control when it issues the token, by the target daemon, or both?"
options:
  - id: "a"
    label: "Control checks the grant and signs a one-use start ticket; the target daemon checks the ticket"
    how: "Before each start the caller asks the hub for a start ticket naming the board's grant, the target machine, the caller's machine, the rung (lead or issue agent), the issue and the recipe. Control checks the grant (the target is one of its machines, under the cap, not lapsed or revoked, the target online) and signs a ticket that lives a minute and is taken once. The caller sends it in the start request over the peer channel it already has (the `agents` machine token, aru-141, #1002). The target checks the hub's signature, that the ticket is for it, from the channel's caller, of its own account, unused, and that the request matches it, then starts the agent and attaches it to the row with the grant's id, so control counts it."
    tradeoff: "One more call to control per start, and a new token kind. A target can't start an agent control didn't allow, but it trusts control's word on the grant, as it trusts its tokens (aru-141)."
  - id: "b"
    label: "Control checks the grant when it issues the `agents` machine token"
    how: "The machine token a daemon asks for to reach another names a board, and control refuses it past the grant."
    tradeoff: "A peer channel is opened once and reused for minutes (#856), and one channel carries the catalog, tasks and starts for every board. A token can't say which start it is for, so the cap can't be counted and a revoke doesn't stop starts on a channel that's open."
  - id: "c"
    label: "The target daemon checks the grant, asking control for it on each start"
    how: "The caller sends the board and issue; the target reads the board's grant from control (with its own boards grant) and counts the agents it started."
    tradeoff: "The cap is across machines, so only control can count it; the target would count only its own. It needs every target to hold a boards grant over the project, and an unchecked caller reaches the start route before anyone has looked at the grant."
  - id: "d"
    label: "The grant is copied to the person's machines, which check it offline"
    how: "Approving the card pushes the grant to each machine it names; a start is checked against the local copy."
    tradeoff: "Revokes and the cap have to reach every machine before they count; offline machines keep stale grants. It duplicates what control already holds, to avoid trusting control, which aru-141 decided to trust."
choice:
  option: "a"
  reason: "Control holds the grant, the board, the revokes and the count across machines, so it decides and signs what it decided; the target checks the signature itself, as it checks machine tokens (aru-141). Jake took the three answers below as recommended (2026-11-24)."
rejected:
  - option: "b"
    why: "A peer channel outlives a start and carries many, so a token can't count the cap or stop the next start after a revoke."
  - option: "c"
    why: "The cap spans machines, so a target can count only its own starts."
  - option: "d"
    why: "It re-creates device keys' distrust of control, which aru-141 gave up."
supersedes: []
evidence:
  - title: "Arugula #1415 (Machines as hands), #1416 (Run this board), #1438 (a lead starts issue agents inside the grant)"
  - title: "Plan: the agent ladder (Doc pdc_7756bb972d172d0c99eb987df68d66a1)"
  - title: "Board: Chief of staff for everyone (brd_e3bdf5fbb0a5e3e334aae27ebd5467c7)"
  - title: "PR #2835, Machine to machine by token: agents and drives (Arugula #1002)"
    url: "https://github.com/arugula-salad/arugula/pull/2835"
decided_by: "jhgaylor"
decided_on: "2026-11-24"
reviews: []
constrains:
  - "aru-049"
  - "aru-050"
  - "aru-141"
  - "path:crates/control-wire/src/machine_token.rs"
  - "path:crates/daemon/src/machine_token.rs"
  - "path:crates/daemon/src/peer.rs"
  - "path:crates/daemon/src/board_attach.rs"
  - "path:crates/arugulad-labs/src/agents/mod.rs"
  - "path:crates/arugulad-labs/src/agents/delegate.rs"
  - "path:crates/arugulad-mcp/src/tools.rs"
---

# A ladder start on another machine carries a start ticket from control

The agent ladder (Doc pdc_7756bb97…) puts a chief of staff and board leads
on any of a person's machines. Each rung starts the rung below it, often on
another of the person's machines: a chief of staff starts a board's lead
where the board's repo is checked out, and a lead starts issue agents on
the machines the board's grant names (#1416, #1438). What shipped gets a
daemon most of the way:

- **Reaching the machine:** a daemon asks the hub for an `agents` machine
  token for another machine and opens a peer channel with it, which answers
  `/api/a2a/` and nothing else, as the caller's account (aru-141, #1002,
  daemon 0.70.98).
- ***Run there*:** `POST /api/a2a/agents/NAME/run` on another machine of
  the same account opens an agent block wearing an *offered* recipe, in the
  project it's offered from (`arugula agents run AGENT --on MACHINE`). It's
  a person's button: it says nothing of what the agent is for, and checks
  only that the caller is the same account.
- **Tasks:** another account's agent sends a task, which waits on a consent
  card on the receiving machine (aru-050).

Missing: a start that says what the agent is for (its board, issue, recipe,
and the room it leads), checked against the board's grant, and an answer
that says when the target is offline.

## Decided: option (a)

**Control decides; the target checks control's word.** It's aru-141's shape
again: control holds the grant, the board, the revokes and the count across
machines, so it decides, and signs what it decided; the target daemon checks
the signature itself, so a relay bug or a confused caller can't start what
control didn't allow. Option (b) can't work because a channel outlives a
start and carries many; (c) can't count a cap that spans machines; (d)
re-creates device keys' distrust of control that aru-141 gave up.

### The start ticket

A JWT the hub signs with its issuer keys, like a machine token, with
`use: "start"` so neither passes for the other:

| Claim | Meaning |
|---|---|
| `sub` | The person's account. The target refuses a ticket of another account: across accounts it's still a task and its card (aru-050). |
| `aud` | The target machine's id. |
| `from` | The calling machine's id. The target refuses it from a channel whose caller is another machine. |
| `grant` | The board's grant (#1416), or none for a lead start the person's own approval is making. |
| `board` | The board. |
| `rung` | `lead` or `issue`. |
| `issue` | The row's issue (`iss_…` or `fi_…`), for `issue`. |
| `recipe` | The recipe it wears, or none for plain Claude Code. |
| `iat`, `exp`, `jti` | Sixty seconds, taken once, as a machine token's are. |

`POST /api/daemon/boards/{board}/start-ticket` (signed as the calling
daemon) asks for one. Control refuses, with a plain sentence the caller can
pass on, when: the board has no grant, or it lapsed or was revoked; the
target isn't one of the grant's machines; the board's live issue agents are
at the cap (counting tickets issued in the last minute that haven't
attached yet, so two quick starts can't both take the last slot); the
caller's machine doesn't hold the board's lead (for `issue`) or isn't the
chief of staff's (for `lead`); or the target is offline.

A start on the caller's **own machine** under a grant asks for a ticket too
and checks it locally, so the cap counts every agent the grant started
wherever it runs. A person's own start (a button, the CLI, an owner's token)
needs no ticket: they are the grant's source.

### What the caller sends

`POST /api/a2a/start` on the target, over the peer channel:

```json
{
  "ticket": "<the start ticket>",
  "board": "brd_…", "rung": "issue", "issue": "iss_…",
  "recipe": "arugula-hand",
  "repo": "arugula-salad/arugula",
  "prompt": "Work Arugula #1415 …",
  "session": "fix-1415",
  "by": { "session": "board-lead", "pane": 41 }
}
```

The target refuses a body whose board, rung, issue or recipe differ from
the ticket's. `repo` says which checkout: the target finds its own clone of
it (the projects it knows, its panes' directories) and opens the agent in a
new worktree there, on a branch named for the issue, as `start_agent`'s
`worktree` does. The caller never names a path on another machine. No
`repo`: the recipe's own project, as *Run there* does.

The route isn't behind the `agent-delegation` flag: the grant is the gate.
The machine's **remote input off** switch (aru-141's third guard) refuses
ladder starts too, since they type into a terminal there.

### What the target does and keeps

It opens the agent block with `start_agent`'s own path, so nothing new
decides where or how it runs, then:

- **`issue`:** attaches the block to the issue's row (`attach-block`, as
  its own machine's block, with its own `boards.write`), with the grant's
  id and who started it. The new session holds the row, so the issue's
  thread reaches it (#765).
- **`lead`:** takes the board's lead for the new session (`lead_board`), so
  the board's thread reaches it.

It keeps the start on the block (`Work`, in its config: board, rung, grant,
and `started_by` machine and session) so a restart keeps it, and writes a
line to the remote sessions log (aru-141's second guard): who, from which
machine, for which board and issue, which recipe, which block. It answers
`{machine, block, row}`.

**Control keeps** the grant on the board (#1416), each ticket it issued
(grant, from, to, rung, issue, when) as the grant's history, and the
attachments, which is how it counts. **The caller keeps** nothing new: the
row says who's on it.

### Offline

The caller looks at the directory's `online` first, and control refuses a
ticket for a target that's offline as well (409 `offline`), naming the
target and the grant's other machines that are online. A Cloud workspace
that's asleep isn't offline: the channel wakes it (M182). Nothing is
queued: the caller tells its person in the board's thread, or asks again
for another machine the grant names.

### On the board

The block is attached to the row as work, so the row is in Doing and shows
the agent and its machine. The attachment says who started it ("started by
the board's lead on geek"). A lead start shows as the board's lead. The
grant, with its machines, cap, what counts against it and *Revoke*, is
#1416's to show on the board.

### What changes from today

- **Tasks and consent cards (another account)** are unchanged. A start
  ticket is always the target's own account's, so another account can't
  use one, and there's no card on the target: the board's card was the
  consent.
- ***Run there* (a person's)** is unchanged.
- **Ladder starts are new:** they carry a purpose and a ticket; they are
  checked against a board's grant, not "this is the same account"; and
  they may wear any recipe the target has, not only an offered one,
  because offering is sharing with the team, which a ladder start isn't.
- **No passkey** on the grant. aru-099's run grant is passkey-signed
  because a hosted box trusted control less. Since aru-141, control can
  already open a shell on every machine of the account, so a passkey on a
  grant that only starts that account's own agents on its own machines
  would guard nothing control couldn't do anyway.

## Jake's answers (2026-11-24)

1. **Recipes:** a ladder start may wear any recipe the target has, its
   user's or its projects', offered or not. Not only offered ones, and not
   only ones the grant names.
2. **A revoke** refuses the next start. Agents already running under the
   grant keep running; a person stops them as they would any agent.
3. **Offline:** refused at once, naming the grant's other machines that are
   online. A Cloud workspace that's asleep counts as online and is woken.
   Nothing waits for a machine to come back.

## Order

1. This record.
2. The ticket's claims in `control-wire`, the daemon's check
   (`machine_token.rs`), `POST /api/a2a/start`, and `start_agent {machine}`
   for an agent, with an integration test on two dev daemons and a fake
   control that issues tickets against a test grant.
3. The ticket route and its grant checks in control (arugula-salad/cloud),
   once #1416's grant exists; until then control refuses every ticket, so
   nothing starts on another machine without a grant.
4. #1416 (the card and the grant) and #1438 (the lead's starts inside it)
   build on these.

Where: `crates/control-wire/src/machine_token.rs` (the ticket's claims),
`crates/daemon/src/machine_token.rs` (its check), `crates/daemon/src/peer.rs`
(the channel), `crates/arugulad-labs/src/agents/` (the start route, beside
*Run there*), `crates/daemon/src/board_attach.rs` (the attach with the
grant), `crates/arugulad-mcp/src/tools.rs` (`start_agent`'s `machine`); the
ticket route and the grant's checks are in arugula-salad/cloud.
