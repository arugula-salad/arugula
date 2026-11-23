# Remote typing for a solo machine: why it's on by default

Arugula issue: https://app.arugula.io/issues/iss_d8ce0c357cf0902c51dec39378ef7a1b

## What changed

Before, joining Arugula cloud asked "May people type into this machine
through Arugula cloud?" *before* sign-in (Getting started's cloud step, and
`arugulad join` on a terminal), and an unanswered join wrote
`remote_input: false`. Someone who joined to answer an agent from their
phone found they couldn't type into their own pane from it.

Now nothing asks. The default is decided once the join lands, when the
daemon knows where it went (`control::remote_input_as_joined`,
`sharing::joined_sets`), and again whenever control moves the machine into
a team (`control::take_move`, `control::remote_input_as_moved`):

| What happened | Owner said before? | `remote_input` |
| --- | --- | --- |
| Joined the person's own account | no | left unset, so **on** |
| Joined a team | no | written **off** |
| Moved into a team (from none, or another) | no | written **off** |
| Any of these | yes (`arugulad sharing remote-input`, `--remote-input`, an older join's answer) | **unchanged** |

`arugulad join --remote-input on|off` (and `ARUGULA_REMOTE_INPUT`) still
set it up front, and `POST /api/setup/control` still takes `remote_input`
only at the machine (`Cx::at_the_machine`), never through control or the
tailnet.

## Why on is safe enough for a personal machine

- **Nobody new gets in.** On a personal machine, the only person whose
  machine token carries an owner or editor role is the account's own
  holder. Other people reach it only through sessions the owner explicitly
  shares, with the role the owner chose when sharing. Remote input doesn't
  widen who reaches the machine; it decides whether those already let in
  may type.
- **It's what the person signed in for.** The flow that leads here is
  "get this on your phone": answering a waiting prompt *is* typing. A
  default of off made the feature fail silently for the person it was
  built for, and invited them to work around it in ways we can't see.
- **The trust it relies on was already there.** With remote input off,
  the owner could still *read* every pane through control, and control
  could read the traffic it relays (aru-141; the Getting started copy says
  so). Typing adds the ability to act, which is real: whoever controls
  the account, or control itself, could run commands. That risk is the one
  aru-141 already accepted ("What it costs"), bounded by short machine-bound
  tokens, the remote sessions log, and the switch.
- **The switch is still the machine's alone.** `arugulad sharing
  remote-input off`, run on the machine, holds everyone coming through
  control to a viewer and closes open channels within seconds
  (`e2e.rs` `RECHECK`). Nothing through control can turn it back on
  (it's not in `SharingSetRequest`), so a compromised account or control
  can't undo an owner's off. Getting started's cloud step now says how to
  turn it off before the person joins.
- **No existing machine changes.** Only a fresh join's save sets the
  default, and only when the file says nothing. A machine joined under the
  old flow with "No" (or unanswered) keeps `remote_input: false`.

## Why a team stays off

On a team's machine, other people's tokens can carry owner or editor roles
(a team owner counts for the team's project sessions, Arugula #1180), so
"on" would let people other than the machine's owner type. That deserves
a question, asked when the machine joins a team; that question is
post-launch. Until then, an unanswered team join writes `off`, as the old
flow did, so the team path keeps its behaviour. The off is written to
`sharing.json` *before* `control.json`, so the daemon never accepts a
control channel on a team machine while typing is on.

The same holds when a personal machine is later moved into a team.
Control's refresh hands the daemon the move; `take_move` says whether
it put the machine into a team, and if the owner never said, the refresh
writes `off` before it saves the move or builds the new state from it.
If `sharing.json` can't be written, the refresh fails and the move waits
for the next one, so the machine is never in a team with typing on unless
its owner chose that. A move out of a team, a replayed move or a refused
one changes nothing.

This applies only at a join or a move. A team machine that joined before
the join question existed (Arugula #1194) and never set `remote_input`
is still on, as it was, so this change flips no team machine already set up.

## Known gaps (for the post-launch team question)

The team question, when it's built, asks at the moments above where this
writes `off`. Until then:

- **A machine that was in a team and rejoins, or is moved back to, its
  owner's account** keeps the team's `off` (it was written, so it stands). Safe, but the owner must
  run `arugulad sharing remote-input on` to type from their phone.
- **A machine whose owner answered "No" under the old flow** stays off for
  the same reason.
