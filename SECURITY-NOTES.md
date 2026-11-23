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
`sharing::joined_sets`):

| Where the machine went | Owner said before? | `remote_input` |
| --- | --- | --- |
| The person's own account | no | left unset, so **on** |
| A team | no | written **off** |
| Either | yes (`arugulad sharing remote-input`, `--remote-input`, an older join's answer) | **unchanged** |

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

## Known gaps (for the post-launch team question)

- **A personal machine later moved into a team** (`take_move`) keeps
  remote input on if it was left unset. The move happens on control's
  refresh, not through a join, and this change doesn't touch it. The
  team question should ask then; until it does, teammates with an editor
  role on a shared or team-project session could type there.
- **A machine that was in a team and rejoins its owner's account** keeps
  the team's `off` (it was said, so it stands). Safe, but the owner must
  run `arugulad sharing remote-input on` to type from their phone.
- **A machine whose owner answered "No" under the old flow** stays off for
  the same reason.
