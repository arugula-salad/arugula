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
The web client and the app will ask once (M103), and so will the `arugula`
CLI (M105). Both write the same file, through the `telemetry.get` and
`telemetry.set` operations (`GET` and `PUT /api/telemetry`, owner only).

- **Yes** makes a random install id: 16 hex digits, unrelated to anything
  else on the machine. Reports carry it.
- **No** deletes the install id, today's counts and any report not yet sent
  (`telemetry.counts.json`). A later yes gets a new id, so reports before and
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
prints the next one as it would go:

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
      "clients": { "web": 4, "tui": 1, "mcp": 2 },
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
  terminal, MCP), and which Labs flags are on. Restored panes and blocks
  aren't counted again. Views (`views`, M103) and crashes (M102) come later.

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
