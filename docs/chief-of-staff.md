# The chief of staff

The chief of staff is one agent per person per project. It watches what
happens in the project and tells you what matters (Arugula #1276). The design
is in the Docs "Chief of staff on the event log" and "Plan as home".

It's an agent with standing instructions, not a new kind of thing. Its
instructions are a recipe, which is also its first prompt, so you can read
what it was told. There are two, one for each place it can run, because each
names the tools it has there:

| Recipe | Runs as | Its tools |
|---|---|---|
| [`chief-of-staff.md`](../crates/arugulad-control-tools/src/recipes/chief-of-staff.md) | an agent block on one of your machines (a Cloud workspace is one more machine) | an agent block's: `post_thread`, `read_thread`, `forge_read`, `issue_write`, `doc`, `board_read`, `board_write`, `link`, `watch` |
| [`chief-of-staff-hosted.md`](../crates/arugulad-control-tools/src/recipes/chief-of-staff-hosted.md) | a hosted agent session on the project's Cloud workspace | a hosted session's: `post_session_thread`, `read_session_thread`, `read_session_issues`, `create_session_issue`, `read_project_doc`, `board_read`, `ask_approval`, `watch` |

The machine's is the chief of staff from now on: it runs on whichever machine
you pick, and nothing needs a Cloud workspace (Arugula #1337). The hosted one
stays while hosted chiefs of staff still run; `session_start` starts it. A
test in the binary (`whole_tests.rs`) renders the machine's recipe and checks
that each tool it names, and each `tool` kind `kind`, is listed for an agent
block.

## On a machine

It runs as an agent block. `start_agent` starts it (Arugula #1338), and the
daemon wakes it when it's idle, with nobody typing in its pane (Arugula
#1339). Each wake is one turn, and it differs from the hosted one in three
ways (below). Two things wake it:

- **Your post in your staff room.** It leads the room, so control tells its
  machine, naming its session (`agents.mention` with `session`), and the
  daemon sends the post to the block named that, as its next prompt.
- **One of its watches firing.** A watch belongs to its block's Claude Code
  session: control sends the watch's one message for that session, and the
  daemon gives it to the block that holds it.

A message for an agent block that isn't in a turn waits about a second for
others (`agent_mentions`' `WAKE_SETTLE`), and all that came by then go in as
one prompt. One that comes while it's in a turn waits in its queue for the
turn to end, as any follow-up does.

When its machine is offline or asleep, control knows: the Overview's staff
tab says **asleep: <machine> is offline**, from the fleet's status. Your
posts stay in the staff room, and control keeps each one it couldn't tell,
and each watch's message, as `offline`. When the daemon is back on the relay,
control sends them all at once (`redeliver`, the newest 20 of the last day,
and a watch's message within its window), so it wakes once for all of them
and catches up from its cursor. `crates/arugulad/tests/integration/chief_of_staff_wakes.rs`
shows both wakes on an idle block and the one wake on return, against a
fake control.

It differs from the hosted one in three ways:

- **Its notes** are in `~/.chief-of-staff/<project>.json` on the machine, one
  file per project, since the machine's home is every project's.
- **Its own events.** It learns who it is on its first wake: it posts in
  your staff room and reads the post back, whose `agent` gives its machine
  and session name, and `capabilities` gives its pane. It skips an event
  whose actor is an agent on its machine with its session name in
  `data.agent_session`, or whose `data.machine` and `data.pane` are its own
  block's (its own `session.needs_input` and `session.idle`).
- **Its approval cards act on its machine** (Arugula #1417). Its
  `ask_approval` asks to start an agent block on an issue, or to close
  agent blocks, on the machine it runs on; Approve does it there (see
  "Approval cards" below).

It never watches `session.idle` for the project, as the hosted one never
watches `session.settled`: its own turns end too.

### On a machine: starting it

The Overview's **Start** does it for you (Arugula #1341): with no staff room
yet, its Chief of staff tab says what the chief of staff does and offers your
machines that the page reaches and whose daemon says `chief_of_staff` in its
features (a Cloud workspace of the project is one more, never the default).
The default is the machine the page is on, if it can. Start calls
`start_agent` there over the page's own connection (`POST /api/mcp/call`, as
the machine's owner). Leading the staff room, one of your DMs, needs the
machine's `threads.read` and `threads.post` over your DMs: if it lacks them,
the tab asks you on a card first and grants them as you. Or ask an agent on
the machine to call `start_agent` with `recipe: chief-of-staff`. Add `project` if it isn't the current
project, and `prompt` for anything to tell it after its instructions (it may
be empty). It needs no Labs flag, and runs the same on a Cloud workspace's
daemon as on any machine. That call:

1. starts a Claude Code agent block titled "Chief of staff · <project's
   name>" (or the `title` you give), in the project: its `staff` is that
   project's staff room;
2. sends it the machine's recipe, then your `prompt`, as its first prompt;
3. makes it your staff room's lead there, by its session's name (an agent
   block's session is named by its title), as its own `post_thread`
   `room: staff`, `lead: true` would. The answer's `chief_of_staff` says
   whether it leads.

`start_agent` takes the lead itself rather than leaving it to the first
wake: a post of yours in staff reaches it from the start, and the first wake
finds itself the lead. If control refuses the lead (no `threads.read` grant
over the project, say), the block still starts, the answer says why, and the
recipe tries again on its first wake.

It refuses a chief of staff on an issue, in a worktree, on a VM or beside
another machine's pane: the lead is a session of the machine that asks
control, so call `start_agent` on the machine it is to run on.

The project's name is in the title because a staff room's posts reach its
lead by its session's name, and an agent block's session is named by its
title (Arugula #1340): a machine with a chief of staff for two projects needs
two names. Two projects with the same name would still share one; give one
a `title`.

### Moving it

There's one chief of staff per person per project, and it stays on the
machine it was started on until it's moved (Arugula #1340). Starting one
where another already leads your staff room moves it: `start_agent` reads the
room's lead first, and then:

| The lead was | What happens to it | Where the new one's cursor comes from |
|---|---|---|
| a block on another machine | Before taking the lead, `start_agent` posts in staff that it moves, naming it. The old one still leads then, so control tells it. It posts `Standing down … cursor N, briefed M` in staff, cancels its watches, deletes its notes file and closes its own block | that post. If it isn't there yet, the new one starts from the log's head, says so, and reads the events in between when the post reaches it (it leads staff now, so it does). If it never comes, the briefings say events before then weren't read |
| a hosted session (`ses_…`) | stopped with `session_stop`, its reason saying where it moved. If that's refused (no `sessions.reply` or `sessions.start` grant), the answer says to stop it from its card | nowhere: its notes stayed on its Cloud workspace. The new one starts from the log's head and says so |
| a block on this machine | closed before the new block opens, as is any agent block here going by the new one's name | its notes file, which is the same file |

The first prompt says which, after the standing instructions, so the new one
knows where to look. The answer's `chief_of_staff.moved_from` says who led
before and `how` it stood down (`told`, `not_told`, `stopped`,
`not_stopped` or `here`), and `closed` lists the blocks closed here.

On each wake the chief of staff also checks it still leads (`read_thread`
`staff`): one whose machine was offline when it was told stands down at its
next wake instead. The lead control keeps is the one that counts, so
there's never more than one lead; only the handover of the cursor waits on
the old one.

### Building boards

Ask it to turn a conversation or a Doc into work and it writes the board
itself; you edit it after (Arugula #1442). It:

1. reads the Doc (or the room) through, and checks no open board already
   has that goal;
2. makes the board with `board_write` `create_board`: a title, a goal that
   says what you can do when it's done, and one group per phase of the Doc,
   in its order, each item a new issue whose body ends with a "Done when" line and names the Doc;
3. sets blockers with `link` (`blocks`), only where one issue needs another
   closed first, inside a phase or across them;
4. puts the Doc in the board's Docs strip (`link_doc`);
5. lets go of the board's lead (`lead_board`, `release`): making a board
   makes its session the lead, and the chief of staff doesn't run boards;
6. posts the board in your staff room, groups, issues and blockers, and
   says it has no lead yet.

It starts nothing. It offers to run the board, and asks for a **Run this
board** card only when you say yes (see "Run this board" below). It changes a board it
made while that board has no lead, when you ask, and never changes a board
another lead runs, except to add a row you asked it to add.

## Hosted: starting it

Ask one of your agents to call `session_start` with `recipe: chief-of-staff`.
Add `project` if it isn't the current project. The agent needs two of your
grants: `sessions.start` over the project, and the grant over your DMs, which
is how it opens your staff room.

That call:

1. opens your staff room in the project, if it doesn't have one yet;
2. posts the recipe there as your agent's `@arugula` ask. That starts an
   agent session on the project's Cloud workspace, as any `@arugula` does;
3. makes that session your staff room's lead, so each of your posts in the
   room is its next prompt.

On its first turn it saves its place in the log and arms its doorbells.
Stop it from its card, like any session.

Hosted sessions have no recipes of their own. A run spec's `agent` is a name
the box refuses for anything but Claude Code, and nothing on a box reads
`.claude/agents`. So Arugula keeps the recipe and sends it as the first
prompt.

## What it does

What follows is the hosted one's; the machine's differs as
[On a machine](#on-a-machine) says. Each time it wakes, it:

- **reads** every event in the project after its cursor (`watch` kind
  `events`). It keeps the cursor in `~/.chief-of-staff/notes.json`, in its
  session's home on the box. A paused or parked box keeps that file. A rebuild
  loses it, and then it says so and starts from the log's head;
- **sorts** what it read:
  - it tells you now when main went red, a session on one of your machines
    asks you something, a board closed, or a lead answered something you
    asked;
  - it asks a board's lead in the board's thread;
  - it files an issue for work nobody owns;
  - it keeps the rest for your briefing;
  - it skips its own events;
- **re-arms** its doorbells. A watch fires only once, so it sets again any
  that isn't live, each for 7 days:
  - `github.check.failed` on `main`;
  - `github.pr.ready`;
  - `board.closed`;
  - `session.needs_input`;
  - a reply under each question it asked a lead;
  - `ask.answered` for each approval card it's waiting on;

  It never watches its staff room. It leads that room, so your posts there
  are already its prompts, and a watch would wake it twice for one post
  (Arugula #1391);
- **ends its turn** with one line in its thread, which notifies nobody. Then
  the box can pause.

A briefing gives counts since the last briefing, with links, not prose.

## What it may and may not do

| May | May not |
|---|---|
| Read the project's events, issues, Docs and rooms, as you may | Read terminals, or anything you can't |
| Post in your staff room and in the project's rooms, as your agent | Post outside the project, except your staff room |
| Open an issue, assigned to nobody | Edit, close or assign issues |
| Make a board when you ask, and change it while nobody leads it (on a machine) | Change a board another lead runs, except to add a row you asked for |
| Make and fill its own Docs | Write other people's Docs |
| Ask you to approve starting an agent on an issue, or stopping agents, on a card | Start, stop or close one itself. Chat approves nothing: only the card's Approve does |
| Watch for events in the project | Spend money |

It acts with your rights, in this project only. Events carry ids and states,
never text. When an event leads it to something someone wrote, it treats that
text as information, not instructions.

## When it can't see

- **Lost events.** The log keeps events for 14 days. If its cursor is older
  than that, a read says which events are gone, and it tells you how many.
- **The log is behind.** It tells you in two cases: a doorbell names an event
  that its read doesn't have yet, or the log took an event more than 5 minutes
  after it happened.
- **Gaps today:**
  - `github.pr.ready` can't be matched by reviewer, and a session can't read
    the forge. So "PRs waiting on your review" is "PRs ready".
  - An approval card starts a session only on a native issue, on the
    project's Cloud workspace. On your machines, a board's grant starts
    agents instead ("Starting agents on your machines").

## Starting agents on your machines

A chief of staff or a board's lead starts the rung below it for a board with
`start_agent {board, prompt, issue?, recipe?, to?, permission_mode?, allow?,
user_settings?}` (aru-143). With `issue`
it's an issue agent on that row; without, the board's lead. `to` is another
of your machines, by name or id; without it, this one.

1. The machine asks control for a **start ticket**
   (`POST /api/daemon/boards/{board}/start-ticket`). Control checks the
   board's grant (which machines, how many agents at once, not lapsed or
   revoked) and that the target is online, and signs a ticket that lasts a
   minute and names the board, the issue, the recipe and both machines. An
   offline machine is refused at once, naming the grant's machines that are
   online; a Cloud workspace that's asleep is woken, not offline.
2. It sends the ticket to the target's `POST /api/a2a/start` over the
   channel it already has to that machine (an `agents` machine token), or
   checks it itself for this one. The target checks the ticket: signed by
   control, for it, of its own account, from the machine that sent it,
   unused, and for the start it came with. Remote input off on the target
   refuses it.
3. The target opens Claude Code in a new worktree of its own clone of the
   caller's repo, in a tab of its own, as session `issue-N` or `lead-…`,
   and titled by that name, so it leads its issue's thread (or is found on
   the board's) by it from the start, not after its first turn. Its
   first prompt is its rung's recipe (Arugula #1440: `board-lead` or
   `issue-agent`, below), a line naming its board and issue, then the
   prompt. It wears `recipe` when that's any other (a Claude Code agent the
   target has), and starts with the permission mode and allow list given
   (never a mode that skips every check). An issue agent is attached to its row with the
   ticket, so control counts it against the grant, and holds the row, so
   the issue's thread reaches it. A lead takes the board's lead.
4. The block keeps who started it, from where, for which board and grant
   (`work.ladder` in its config), and the target's remote sessions log gets
   an `agent` line.

Issue agents count against the grant's cap from their ticket until control
hears their block closed. A start past the cap is refused, naming it
("allows 2 issue agents at once, and 2 are running").

The lead stops an issue agent with `close {pane}` here, or `close {pane, to}`
on another of your machines (Arugula #1438). The stop goes to the target's
`POST /api/a2a/stop` over the same channel, with no ticket: a stop only frees
what a start took. The target closes only a block a ladder start opened
there (`work.ladder`), and only for the machine that started it or the
board's current lead, as control says it. Either way the machine tells
control at once that the block closed, without waiting for its next
block-state report, so the slot is free for the lead's next start. The
answer says so. Here, each issue agent is in a tab of its own, which an
agent's token doesn't reach, so `close` lets the lead past that for a block
a ladder start opened for its board (`work.ladder`), when control says the
caller's own pane on this machine leads it (Arugula #1471). Any other pane in
another tab stays refused, and so does a sibling issue agent. A guest's agent can't use `start_agent` with `board` or
`close` with `to`.

Revoking the grant refuses the next start; agents already running keep
running. Another account's agents still send tasks (docs/labs.md, "Tasks and
consent").

## Running a board

A board's lead and the issue agents it starts run Arugula's own recipes
(Arugula #1440), beside the chief of staff's in
`crates/arugulad-control-tools/src/recipes/`:

- **`board-lead.md`**: the lead reads the board, its Docs and the project's
  rules, and starts one issue agent per ready row with the ladder start
  above (`start_agent {board, issue, prompt}`), so each is a block on its
  row, counted against the grant, in a tab of its own. Each one's prompt
  says its issue, what's on main that it builds on, its siblings and their
  issue threads, the project's rules, and to end each turn with its PR,
  checks, what it tested and whether it needs a server release. The lead
  talks to them in their issues' threads, hears them in the board's
  thread, merges when green by the project's rules, keeps each row's note
  as a one-line status, puts product questions to the person as options,
  and closes the board with what people can now do. It never does ticket
  work in sub-agents nobody sees. A start refused past the board's grant
  (its cap, a machine it doesn't name, no grant) sends the lead to stop
  any agent that's done or stuck (`close {pane, to?}`), then to post once
  in the person's staff room to the chief of staff, naming the board,
  what it needs and why; the chief of staff asks the person with a new
  **Run this board** card, and answers in the board's thread. An offline machine is no such case: the lead
  starts on an online one the refusal names.
- **`issue-agent.md`**: an agent started on an issue leads that issue's
  thread, and a post there is its person's or its lead's word. It reports
  in the board's thread, reaches its siblings in theirs, keeps its row's
  note, and doesn't merge.

Every start on an issue puts `issue-agent` first: a ladder start, an
approved card's start, and `start_agent {issue}` for Claude Code or Codex
(any ACP agent server may have none of Arugula's tools). A ladder start for
a board's lead puts `board-lead` first, and so does `start_agent {recipe:
"board-lead"}` on a machine, with no Labs flag; naming either as `recipe` is
the same as not, and neither is worn as a Claude Code agent. **Run this
board** (Arugula #1416) starts the lead with the ladder's lead start.

## For agents: a session's rooms

An agent session's MCP (`arugulad-mcp`'s `session`) has two room tools:

- `read_session_thread {room, thread?, after?, limit?}` reads a room the
  asker may read:
  - `staff`, the asker's staff room in the project;
  - a room of the session's project: a board's or an issue's thread (by the
    object's id, such as `brd_…`), or a channel.
- `post_session_thread {room, text, parent?}` posts in the same rooms, as the
  asker's agent, labelled with the session. The post reaches whoever an
  agent's post there reaches: the asker in their staff room, a board's lead
  on its thread, a person named by `@login`. The session's machine holds no
  grant, so its posts never prompt or answer a session.

Both tools are ops of `POST /api/hosted/work/session/issues`: `ThreadRead` and
`ThreadPost`.

It reads the project's boards with `board_read {board?}` (Arugula #1381), the
op `BoardRead`, as the asker may read them, and never changes one:

- with no board: each board's title, id, progress and lead (the session a
  post on its thread reaches);
- with `board` (`brd_…`, or an open board's exact title): the board as
  `board_read` prints it anywhere else: its goal and lead, then its rows by
  lane, each with who works it (the session that holds it, agent sessions
  and blocks on it).

## Approval cards

When you ask the chief of staff to "get an agent on #N" or to close some
sessions, it doesn't ask a lead and it doesn't do it itself (Arugula #1278).
It asks you on a card:

1. It calls `ask_approval`: `action` `start` with `issue` (and `prompt` or
   `repo` if you named one), or `action` `close` with `sessions`, plus a
   `why`. The tool is the op `Propose` of `POST /api/hosted/work/session/issues`.
   Control checks the proposal as you: you must be able to see the issue, or
   stop each session.
2. The card is the platform's ask message in your staff room (`via`
   `ask:prp_…`). It mentions you until it's answered, so it's under Needs
   you (`GET /api/projects/{project}/asks`) and pushed to your phone once.
3. **Approve** does it as you, through the same path your own press takes.
   A start is **Agent on this** on the issue, so `@arugula` on it with the
   same checks and the same box. A close is each session card's **Stop**.
   **Deny** drops it. Only you can answer it, and only once. Nothing posted
   in chat answers it.
4. Control emits `ask.answered`, with the card (`ask`), the session that
   asked, the `action` and the `answer` (`approved`, `denied` or `failed`)
   and, for a start, the session it `started`. The chief of staff watches it
   for each card, so your answer wakes it. The card's message gets a line
   saying who answered and what happened.

### On a machine

The chief of staff on one of your machines asks the same way (Arugula
#1417), with the machine's own `ask_approval`, called from its agent block:

1. `action` `start` with `issue` (and `prompt`), or `action` `close` with
   `blocks`: agent blocks on its machine, by pane. The daemon checks each
   block is an agent block there and not the chief of staff's own, then
   `POST /api/daemon/projects/{project}/proposals` (`MachinePropose`: the
   proposal, its session's name and pane, and the blocks as the card shows
   them), signed as the machine. Control checks it as the chief of staff's
   posts in your staff room are checked (`threads.post`).
2. The card is the same: the platform's ask message in your staff room
   (`via` `ask:prp_…`), under Needs you, on your phone, yours alone to
   answer, once. It says which machine.
3. **Approve** sends that machine `agents.approved` (`AskApproved`) over its
   connection to control, and the daemon does it as you
   (`crates/daemon/src/approvals.rs`):
   - **start:** one agent block on the issue, as `start_agent {issue,
     worktree}` would make it: Claude Code with your own settings, in a new
     worktree of the repo the chief of staff works in (on `feat/N-…`), in a
     tab called `issue-N`, attached to the issue's row. A row another agent
     holds refuses it, and so does a chief of staff that's gone from the
     machine since it asked (moved, Arugula #1340, or closed).
   - **close:** each block, as its `close` does: a worktree goes only if
     nothing would be lost.

   It answers `approved` with what it did, in a line for the card, or
   `failed` with why. Remote input off on the machine refuses both. A
   machine that's offline, or too old to know the op, fails the card.
4. `ask.answered` is the same event, with the agent's session name as
   `session`, the `machine`, and for a start the block it `started`
   (`%N`). The chief of staff watches it per card.

A start on another of your machines isn't a card's: it's a ladder start,
with control's start ticket (aru-143, #1415), under the board's grant. The
"Run this board" card gives that grant.

### Run this board

One card starts a board's lead and grants it agents (Arugula #1416).
Approval is at the board, not at every agent. The card comes two ways:

- **Your chief of staff on a machine asks:** `ask_approval` `action`
  `run_board` with `board`, and optionally `machine` (its lead's machine,
  by name or id; default the chief of staff's), `machines` (others its
  issue agents may use), `cap` (issue agents at once; default 3),
  `lapse_days` (default: until the board closes) and `prompt` (for the
  lead). Its recipe offers this after it builds a board, and asks only
  when you say yes.
- **You press Run this board** on a board with no lead: on the Overview's
  Board level, or on the board's page under who leads it
  (`POST /api/boards/{id}/run`). The card has the defaults: the lead on the
  machine your chief of staff runs on (else one of yours that's online), 3
  at once there, until the board closes.

The card is in your staff room, under Needs you and on your phone. It
reads: **Run** the board / **Lead on** a machine / **Issue agents on**
machines **· up to N at once** / **Until the board closes** (or for N days)
/ **Why**. There's one card per board: another is refused while one is open,
and pressing Run this board again finds it.

**Approve** writes the board's grant (`board_grants`: its lead's machine,
the other machines, the cap, the lapse) and sends `agents.approved`
`run_board` to the lead's machine, whichever asked. That machine starts the
lead as a ladder start: a start ticket under the grant, then an agent block
titled "Lead · <board>", in a worktree of the project's repo when it has a
clone, which takes the board's lead (`lead_board`, by that title), so the
board's thread reaches it. The lead starts as the board-lead recipe starts
its issue agents: `permission_mode` `auto`, your own Claude Code settings,
and `allow` `Bash`, `Read`, `Edit`, `Write`, `Glob`, `Grep` and `WebFetch`,
so its commands don't each raise a card. Never a mode that skips every
check. Started, the grant replaces any the board had;
not started (offline, remote input off, too old), the grant is revoked and
the card says why. **Deny** drops it.

The board's page shows the grant under its lead: "Grant: lead on geek ·
agents on geek, jake-mini · up to 3 · until the board closes", with
**Revoke** for you. Revoking (`DELETE /api/boards/{id}/grant`) leaves the
lead and its agents running, and refuses the next start under it. Closing
the board lapses it.

Going past the grant (another machine, more at once) takes a new card. A
lead refused past the grant asks the chief of staff in your staff room; it
decides whether the need is real and, if so, asks you with a new card for
the board. A card for a board that has a lead doesn't start another: Approve
replaces the grant, and the lead runs on where it is.

The chief of staff answers the lead in the board's thread (`post_thread`
with the board's `brd_…` id), never in your staff room (Arugula #1461): it
leads that room, so a post there reaches only itself. A post in the board's
thread reaches whoever leads the board, wherever it runs. It posts there
twice: when the card is up (or why it won't ask), and when you approve or
deny it, so the lead starts the waiting rows without anyone prompting it.
It keeps such cards in its notes (`for_leads`, card to board), since
`ask.answered` doesn't name the board.
