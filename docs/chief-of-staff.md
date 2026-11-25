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
- **No approval cards yet.** A machine has no `ask_approval` (Arugula
  #1417). Asked to get an agent on an issue or to close sessions, it tells
  you where to press **Agent on this** or **Stop**, and does neither itself.

It never watches `session.idle` for the project, as the hosted one never
watches `session.settled`: its own turns end too.

### On a machine: starting it

Ask an agent on the machine to call `start_agent` with
`recipe: chief-of-staff` (the Overview's **Start** will call it on the
machine you pick, Arugula #1341). Add `project` if it isn't the current
project, and `prompt` for anything to tell it after its instructions (it may
be empty). It needs no Labs flag, and runs the same on a Cloud workspace's
daemon as on any machine. That call:

1. starts a Claude Code agent block titled "Chief of staff" (or the `title`
   you give), in the project: its `staff` is that project's staff room;
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

It starts nothing. Running a board will be the **Run this board** card
(Arugula #1416), which it will offer once that exists. It changes a board it
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
| Ask you to approve starting a session on an issue, or stopping sessions, on a card (hosted only, for now) | Start, stop or close one itself. Chat approves nothing: only the card's Approve does |
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
`start_agent {board, prompt, issue?, recipe?, to?}` (aru-143). With `issue`
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
3. The target opens Claude Code (wearing the recipe, any recipe it has) in a
   new worktree of its own clone of the caller's repo, as session
   `issue-N` or `lead-…`. An issue agent is attached to its row with the
   ticket, so control counts it against the grant, and holds the row, so
   the issue's thread reaches it. A lead takes the board's lead.
4. The block keeps who started it, from where, for which board and grant
   (`work.ladder` in its config), and the target's remote sessions log gets
   an `agent` line.

Revoking the grant refuses the next start; agents already running keep
running. Another account's agents still send tasks (docs/labs.md, "Tasks and
consent").

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
