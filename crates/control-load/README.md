# arugula-control-load

The load test of control (#530): people, their machines and their pages,
against a staging copy of control, through the routes the page and
`arugulad` use. Not shipped: no release builds it. The staging copy, and
bringing it up and down, are in [docs/control-ops.md](../../docs/control-ops.md),
"Load testing".

What each fake does:

- **A person** signs up with a passkey made in software (`attestation:
  none`, Ed25519), as a new browser does: the session, the browser as the
  account's first device, then a new page's first calls, the platform's
  (`/api/projects`, `/api/inbox`) among them. `src/account.rs`.
- **A machine** joins that account (its proof, the browser's approval of
  its code, its poll), dials `/api/relay/dial` signed with its key, and
  serves the mux as `arugulad` does. On each channel a page opens, it
  writes a pane's output: a busy pane's pace (`--busy-rate`, 4,000 small
  messages a second, #443) on `--busy` of them, an idle pane's on the
  rest. Control relays Noise messages it can't read, so opaque bytes of the
  same size and pace load it the same. Every minute it makes a daemon's
  refresh (`/control.json`, its certificates, its peers). It dials again
  as `arugulad` does, backing off to a minute. `src/daemon.rs`.
- **A page** opens `/api/relay/m` with a channel to each of its person's
  machines (M25), refreshes devices and the directory every 10 s and the
  platform's calls every 30 s. `src/browser.rs`.

What it measures (`src/stats.rs`), as JSON lines in `--out`: every HTTP
call's latency by name; `page.channel_open` (asking for a machine's
channel until control says it's open: what a tab waits for);
`page.echo` (a message to the machine and its echo, through whatever pane
output is queued ahead of it: a keystroke's echo); `daemon.dial`; counts of
refusals by status or close reason; what's open; pane messages a second;
and the generator host's load.

```sh
scripts/server-cargo build --release -p arugula-control-load
nice -n 19 $CARGO_TARGET_DIR/release/arugula-control-load \
  --url https://arugula-control-loadtest.fly.dev --out run.jsonl \
  run --steps 100,1000,5000 --hold 300 --state state.json
nice -n 19 $CARGO_TARGET_DIR/release/arugula-control-load \
  --url https://arugula-control-loadtest.fly.dev --out burst.jsonl --concurrency 64 \
  signup --accounts 500
crates/control-load/fly-watch.sh > fly.jsonl   # beside it: what Fly sees
```

`--state` keeps the people and machines (their keys and sessions) for the
next run, so a ramp needn't sign up and enrol again; keep it out of the
repository. It refuses production's URLs. A staging control needs the
generator's address in `ARUGULA_LOADTEST_EXEMPT_IPS` (a `loadtest` build of
control), or the per-address limits stop it at 10 sign-ups and 30 joins an
hour.
