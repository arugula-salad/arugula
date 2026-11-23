# The chief of staff

The chief of staff is one agent per person per project. It watches what
happens in the project and tells you what matters (Arugula #1276). The design
is in the Docs "Chief of staff on the event log" and "Plan as home".

It's an agent session with standing instructions, not a new kind of thing.
Its instructions are
[the recipe](../crates/arugulad-control-tools/src/recipes/chief-of-staff.md).
The recipe is also its first message in your staff room, so you can read what
it was told.

## Starting it

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

Each time it wakes, it:

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
| Make and fill its own Docs | Write other people's Docs |
| Suggest starting, stopping or closing a session | Start, stop or close one. Chat approves nothing, and approval cards are #1278 |
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
  - There's no `ask.answered` event for your answer on an approval card yet
    (#1278).

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
