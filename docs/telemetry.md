# Telemetry

Arugula can send usage counts from your machine to Arugula. It sends nothing
until you say yes, and you can see exactly what it would send before and
after you do. This page says what is sent, when, where, and how to turn it
off. The plan behind it is #989.

## Yes, no, and what keeps it off

Nobody has answered on a new install, and that means off. The answer is a file
in the daemon's state directory, `telemetry.json`:

```sh
arugulad telemetry status          # on or off, why, and the next report
arugulad telemetry status --json   # the same, as JSON
arugulad telemetry on
arugulad telemetry off
```

They work with the daemon stopped. A running daemon follows within a minute.
`arugula telemetry on|off|status [--json]` does the same through the running
daemon, and on the file when no daemon runs.

The web client and the desktop app ask the machine's owner once, with a card
in the corner (M103), and the `arugula` CLI asks once (below). Both write the
same file, through the `telemetry.get` and `telemetry.set` operations (`GET`
and `PUT /api/telemetry`, owner only). The card's *What's sent* shows the
preview below. Closing it without answering asks again on the next load.
Control's page asks about the account first (M104). Once that's answered (or
from a control that doesn't ask), each machine the person owns asks once, as
it's shown there. A read-only link never asks. *Usage and crash reports…* in
the machine's settings changes the answer any time (on a phone, in the
sheet).

### The CLI's question

The first `arugula` command a person runs at a terminal asks, on stderr,
before the command runs:

```
Help improve Arugula? Send usage counts and crash reports from this machine (never terminal contents, file names, commands or prompts). Details: arugula telemetry status. [y/N]
```

`y` or `yes` is yes. Enter, or anything else, is no. Either answer is saved
(`via: cli`) and the CLI never asks again: through `telemetry.set` when the
daemon runs, or straight into `telemetry.json` when it doesn't. If saving
fails, it says so in one line and runs the command anyway.

It asks only when all of these hold, and otherwise says nothing:

- stdin and stderr are terminals (not a pipe or a script);
- it talks to the local daemon (no `--host` or `--ssh`), and the daemon says
  nobody has answered (it refuses agents and guests);
- nothing keeps telemetry off (`DO_NOT_TRACK`, `ARUGULA_TELEMETRY=off`, `CI`,
  a test run);
- the command isn't `--json`, `arugula mcp`, `arugula events`, `arugula
  telemetry`, or one of Claude Code's hooks;
- no agent runs it (`CLAUDECODE` or `AI_AGENT` set).

The install scripts say the first
`arugula` command will ask, and that `ARUGULA_TELEMETRY=off` means it never
will (they leave that out when `ARUGULA_TELEMETRY` or `DO_NOT_TRACK` is set).

- **Yes** makes a random install id: 16 hex digits, unrelated to anything
  else on the machine. Reports carry it.
- **No** deletes the install id, today's counts and any report not yet sent
  (`telemetry.counts.json`), and the crashes waiting (`crashes/`). A later yes gets a new id, so reports before and
  after can't be joined up.

Telemetry is off, whatever the file says, when any of these holds:

- `DO_NOT_TRACK` is set (to anything but `0`);
- `ARUGULA_TELEMETRY` is `0`, `off`, `false` or `no`;
- `CI` is set;
- it's a test run (nextest, and the test harness's daemons, which set
  `ARUGULA_TELEMETRY=off`).

While it's off, nothing is counted either.

## What is sent

One kind of report, typed in `crates/control-wire/src/telemetry.rs`. Every
field is a number or a value from a fixed list: no names, paths, commands,
titles, prompts or error messages fit in one. `arugulad telemetry status`
prints the next one as it would go. Before a yes it prints the first one a
yes would send: this machine's real version, OS, arch, channel and control,
no counts, `"install": "<made when you say yes>"` and `"preview_only": true`.

`telemetry.get` and `telemetry.set` are for the owner in person. An agent's
MCP token, a read-only token and the CLI run by an agent are refused
(`arugula telemetry` refuses an agent itself, before it reaches the daemon or
the file).

Once on, the preview looks like this:

```json
{
  "install": "3f9c0a1b7d2e4c58",
  "version": "0.43.0",
  "os": "linux",
  "arch": "x86_64",
  "channel": "script",
  "control": "hosted",
  "events": [
    { "event": "opted-in", "via": "cli" },
    { "event": "started", "service": true },
    {
      "event": "daily",
      "panes_opened": 14,
      "panes_peak": 6,
      "agents_started": { "claude": 3 },
      "blocks_opened": { "terminal": 14, "browser": 2, "review": 1 },
      "clients": { "web": 4, "cli": 1, "mcp": 2 },
      "views": {},
      "flags_on": ["chat"],
      "web_errors": 0
    }
  ],
  "crashes": []
}
```

- `channel`: how Arugula was installed, from where `arugulad` is (`script`
  for `~/.local/bin`, `homebrew`, `desktop`, `source` for a build, `other`
  when unsure).
- `control`: whether the machine is joined to the hosted control, someone
  else's, or none. Never which one.
- **`opted-in`**: sent right after a yes (within a minute of `arugulad
  telemetry on`), with where it was given.
- **`started`**: a minute after the daemon starts, and whether it runs as a
  service.
- **`daily`**, once a day: terminals opened and the most open at once (taken
  when one opens), agent blocks started by runtime, blocks opened by kind,
  clients that attached by where (the web client, a phone, the CLI's
  terminal, MCP), and which flags in Developer settings are on. Restored
  panes and blocks aren't counted again. `views` are the views the web client opened
  (swarm, chat, conversations, agents, palette, settings, getting started,
  share), and `web_errors` how many errors our code hit there:
  the page sends what it counted to its daemon each minute and as it goes
  away (`telemetry.note`, owner only; nothing is kept while it's off, and
  one note adds at most 1,000 to a count). Control's page sends its own to
  control instead (M104), never to the machine.
- **Clients** by where: the desktop app's window says it's the app (its
  webview's User-Agent is a browser's), so it counts as `desktop`.
- **`crashes`**: the panics of `arugulad`, `arugula` and the desktop app,
  below.

## Crashes

While telemetry is on, a panic in `arugulad`, the `arugula` CLI or the
desktop app is written down in the daemon's state directory, one file each,
`crashes/<ms>-<process>.json`, and the panic still prints as it always did.
Off, nothing is written. A crash holds only where it happened:

```json
{
  "process": "daemon",
  "version": "0.43.0",
  "location": "crates/mux/src/pane.rs:812",
  "frames": ["arugula_mux::pane::Pane::resize", "arugula_mux::mux::Mux::resize"],
  "at": 1760000000000
}
```

- `location`: the file and line in Arugula's own source, which every build
  knows. A panic inside a library is placed at the first of Arugula's
  functions that called it, in a build with line tables (a debug build); a
  release build says `crates/unknown`.
- `frames`: Arugula's own functions on the stack, innermost first, at most
  32. Nothing from libraries or the system is kept. Only a build that keeps
  its symbols has them. Today `arugulad` and `arugula` keep them (Cargo's
  release default drops only debug info), and the desktop app's own binary
  doesn't (`strip = true`): its crashes carry the location alone.
- **Never the panic's message**, which can hold a path or someone's text.

The daemon sends them with its next report (`started` after a restart, so a
crash that stopped it goes when it's back), at most 10 at a time, and deletes
each file once a report holding it was taken. At most 50 wait: past that the
oldest go, and so does any a week old. A no deletes them all, and `arugulad
telemetry status` shows the waiting ones in its preview.

Counts are kept in `telemetry.counts.json`, saved every hour and when the
daemon stops, so a restart loses at most an hour's.

## Where it goes

To the hosted control, `https://control.arugula.io/api/telemetry`
(`ARUGULA_TELEMETRY_URL` moves it), which passes it to product analytics. No
analytics key ships in Arugula.

- A machine joined to that control signs its report as itself, so the
  report counts for its account. If control refuses the signature (the
  machine was removed, say), the report goes again unsigned, and reports stay
  unsigned until the machine joins again.
- Any other machine, including one joined to someone else's control, sends
  it unsigned: control knows it only by its install id.
- A report that doesn't go (offline, or control busy) is kept and tried
  again an hour later. Reports more than a week old are dropped.
