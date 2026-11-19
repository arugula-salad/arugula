# Feedback

You, or an agent working for you, can tell the Arugula team about Arugula
itself: a bug, something you needed that isn't there, an idea, or praise.
Nothing goes unless someone writes it and sends it. This page says what is
sent, where it goes, and how to turn it off for agents.
[What leaves your machine](what-leaves-your-machine.md#before-the-machine-joins-control)
lists it beside everything else the daemon sends. The plan behind it is
Arugula #1213 to #1220.

## Sending it

- **From a terminal:** `arugula feedback "text"`. The first line is the
  title, unless `--title` gives one (at most 200 characters). `--kind` is
  `bug`, `missing`, `idea` (the default) or `praise`.

  ```sh
  arugula feedback "Panes don't reflow on a phone
  I rotated it and the pane stayed narrow." --kind bug
  ```

- **From an agent:** the `feedback` MCP tool (`kind`, `title`, `body`).
  The server's instructions tell an agent to use it when an Arugula tool
  fails it, lacks something it needs or gets in its way, and to say what it
  was trying to do, never secrets or code. An agent doesn't ask you first.
  `arugula feedback` run by an agent (`CLAUDECODE` or `AI_AGENT` set) counts
  as the agent's.
- **From the web client and the desktop app:** *Send feedback…*, in the
  account menu and the command palette (`feedback.open`). On the hosted
  pages (app.arugula.io) it's in the account menu too, and sends as the
  person signed in.

Each one taken answers with an id such as `F-7K2Q9X`. The body is at most
8 KB.

## What's attached

Only what was written, and a little context:

- the kind, the title, the body, and whether you said you can be contacted;
- a person or an agent, and for an agent its name and version (as its MCP
  client says);
- Arugula's version, the OS and architecture, and how far through Getting
  started you are;
- from the web client, the page you were on, without its query or fragment;
- for an agent, its last failed Arugula tool call: the tool's name and its
  error, cut to 1 KB. **Never its arguments.**

**Never attached:** terminal contents, file names or paths, prompts, code,
or anything else from the machine. A body that looks like it holds a key or
token is stored with it redacted. The types are in
`crates/control-wire/src/feedback.rs`.

## Where it goes

To the Arugula team, through the hosted control,
`https://control.arugula.io/api/feedback` (`ARUGULA_TELEMETRY_URL` moves
it, as it does telemetry's reports). Each one taken becomes an issue in our
own tracker, in the team's Feedback project, with a one-line post in our
`#feedback` channel. Nothing goes to GitHub or another service.

What was written is shown defanged: fenced as quoted text, with no
@mentions, and links and images not rendered, so it can't reach anyone or
steer whoever (or whatever) reads it.

- A machine joined to that control signs its send, so it counts for its
  account.
- Any other machine sends it unsigned, under a random install id: see
  [Without an account](#without-an-account).

Telemetry's answer has no say in any of this: feedback goes when someone
sends it, with telemetry on or off.

## Turning it off for agents

On by default. Off, the `feedback` tool isn't listed, the server's
instructions don't mention it, and an agent's sends (tool or CLI) are
refused. A person's `arugula feedback` still goes.

- *Agents may send feedback to Arugula* in the machine's sharing settings;
- `arugulad sharing agent-feedback on|off` (`agent_feedback` in
  `sharing.json`, in the daemon's state directory);
- `ARUGULA_FEEDBACK=off` (or `0`, `false`, `no`) in the daemon's
  environment, whatever `sharing.json` says. `arugula feedback` run by an
  agent checks its own environment too.

All three are read on every call, with no restart.

## Without an account

A machine that isn't joined to Arugula cloud can still send. Its sends are
anonymous, and guarded so they can't be abused:

- **Proof of work:** the daemon solves a small puzzle for each send, so
  sending many costs the sender something. Control can ask for a harder one;
  past a limit the daemon gives up and says so.
- **Rate limits:** per IP address, per install id, and an hourly cap on all
  anonymous sends together. Over one, you're told to try again later.
- **Held:** a send over the hourly cap, or one a spam check flags, is kept
  but held: it opens no issue and posts nothing until a person on the team
  releases it (or drops it).
- **Paused:** we can stop taking anonymous feedback at any time, with no
  release. While it's paused, a send says so: join the machine
  (`arugula join`) or send it signed in at app.arugula.io.

A signed-in or joined send has its own limits: an agent sends at most 20 a
day from one machine, and the same title from one machine within a day
folds into the first.

## Offline

A send that can't reach control (offline, or control busy) waits in
`feedback.queue.json` in the daemon's state directory, and goes again by
itself, a minute later and then less often, up to once an hour. It keeps the
send as written, with no signature or stamp: those are made again when it
goes. At most 50 wait (past that, the oldest goes), and **a send a week old
is dropped**. A send control refuses (too long, a limit, anonymous intake
paused) isn't queued: you're told why at once. One already waiting that
meets a rate limit waits on.
