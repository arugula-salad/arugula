# Running Arugula control

Operating control: the hosted one at
[control.arugula.io](https://control.arugula.io), running your own, and
testing it. Using
control (accounts, devices, joining machines, teams) is in the public docs:
[docs.arugula.io/control](https://docs.arugula.io/control/). The design,
and what holds if control turns hostile, is in
[control-e2e.md](control-e2e.md).

## Running your own

Only the hosted control is supported for now (#662). The code is open and
these notes are how it runs, but a control you run yourself gets no help.
For a setup of your own, use the tailnet without control.

`arugula-control` is one static binary with an SQLite database. Put TLS
in front of it: Caddy, Fly, or `tailscale serve`.

```
arugula-control --public-url https://control.example.com --listen 127.0.0.1:7690 --db /var/lib/arugula/control.db
```

- **Moving to another URL:** run with `--public-url` set to the new
  one and `--also-url` (`ARUGULA_CONTROL_ALSO_URLS`, comma-separated) to
  the old. Both answer: a browser is served as the site it came in on, with
  that site's own origin, cookies and passkeys (a passkey works only where
  it was made), and its page on an old URL says where control is now.
  Daemons and CLIs work at any of them. Register the GitHub callback for
  each URL (an OAuth app takes one host; a GitHub App takes several). Once
  people have signed in and approved their browsers at the new URL,
  `--also-redirect` (`ARUGULA_CONTROL_ALSO_REDIRECT=true`) sends browsers
  from an old URL's pages to the new one, while daemons, the API and
  sign-ins already under way are still answered there.
- **Sign-in:**
  - **Passkeys** work whenever control has a domain name (WebAuthn refuses
    IP addresses).
  - **GitHub**: register a GitHub App (or OAuth app) with the callback
    `https://control.example.com/auth/github/callback`, then set
    `GITHUB_CLIENT_ID` and `GITHUB_CLIENT_SECRET`.
- **The GitHub App** (optional): forge blocks' live updates from
  GitHub, and read access for hosted boxes with no `gh` login. Make it
  with GitHub's manifest flow (or by hand at *Settings → Developer settings
  → GitHub Apps*):
  - **Permissions**, all read-only: metadata, contents, pull requests,
    issues, checks, commit statuses, actions.
  - **Events:** pull request, pull request review, pull request review
    comment, issues, issue comment, check run, check suite, status,
    workflow run.
  - **Webhook URL** `https://control.example.com/github/webhook`, with a
    secret. **Callback URL** `https://control.example.com/auth/github/callback`
    if it also signs people in.
  - Install it on the accounts (or organizations) whose repositories you
    want live, then give control its id, slug, client id and secret,
    webhook secret and private key: `GITHUB_APP_ID`, `GITHUB_APP_SLUG`,
    `GITHUB_APP_CLIENT_ID`, `GITHUB_APP_CLIENT_SECRET`,
    `GITHUB_APP_WEBHOOK_SECRET`, and `GITHUB_APP_PRIVATE_KEY_FILE` (a
    `.pem` path) or `GITHUB_APP_PRIVATE_KEY` (the PEM itself; `\n` for
    newlines works). On Fly:
    `fly secrets set GITHUB_APP_ID=… GITHUB_APP_SLUG=… GITHUB_APP_CLIENT_ID=… GITHUB_APP_CLIENT_SECRET=… GITHUB_APP_WEBHOOK_SECRET=… GITHUB_APP_PRIVATE_KEY="$(cat app.pem)"`.
  - With no `GITHUB_CLIENT_ID`/`GITHUB_CLIENT_SECRET`, the App's client id
    and secret sign people in with GitHub.
  - **What control keeps of a webhook:** the signature is checked
    (HMAC-SHA256, constant time), the delivery id deduplicated, and only
    `{provider, host, repo, number?, event, delivery}` goes to daemons,
    over their relay sockets. Its log has the event, delivery, repository
    and how many daemons heard; never the payload.
  - **Who hears what:** a daemon's account must have signed in with GitHub,
    and the App's installation for the repository must be that GitHub
    user's, or GitHub must list them as a collaborator on it (asked with
    an installation token; answers kept ten minutes). Both go by the
    numeric GitHub user id, not the login. Organization membership alone
    doesn't count, and passkey-only accounts hear nothing: their blocks
    poll. A machine watches at most 200 repositories and an account 400.
  - **Hosted boxes** ask `POST /api/daemon/github/token {repo}` (signed as
    the daemon) for an installation token scoped to that repository with
    read-only permissions; control keeps one until five minutes before it
    expires.
- **Blocks through control** (M50, optional): `--block-domain`
  (`ARUGULA_CONTROL_BLOCK_DOMAIN`, `arugula.run` for the hosted one) gives
  each block an origin of its own, `b-<key>.<domain>`, so a port or editor
  block on any joined machine shows in control's page
  ([control-e2e.md](control-e2e.md#block-channels)).
  - Use a registrable domain of its own, used for nothing else: every
    block must be cross-site to control's page and to every other block.
    Control refuses to start with control's own host, or a parent or child
    of it.
  - Point a wildcard DNS name (`*.<domain>`) at control, with a wildcard
    certificate: on Fly, `fly certs add "*.arugula.run"`.
  - On those hosts control serves only the bootstrap page, the block's
    worker and shim (`/.arugula/…`, from `web/dist/block/`) and the block
    relay (`/.arugula/relay?d=<daemon>`), never its own page, cookies or
    API. Anything else under the domain is a 404.
  - The block relay has no session (control can't read the grant; the
    daemon checks it). It takes 3000 sockets an hour per client address,
    64 at once per machine and 16 per block origin. A socket that sends
    nothing in 10 seconds, or is idle for an hour, closes. Block sockets
    count against `ARUGULA_RELAY_MAX_TOTAL` and the machine owner's
    traffic, not against the owner's `ARUGULA_RELAY_MAX_SOCKETS`.
  - Control's page may frame `*.<domain>` on its own scheme (its
    `frame-src`), so a local control on plain http frames
    `http://b-<key>.<domain>:<port>`; `blocks.localhost` works locally with
    no DNS (`web/e2e/blocks-control.spec.ts`).
  - Joined machines read `block_domain` from `/control.json` on each
    refresh. With it, port and editor blocks open on them with no block
    flags (no `--block-listen`); their own page doesn't offer those
    blocks, since it can't show them.
- **Billing** (optional) needs both `STRIPE_SECRET_KEY` and
  `STRIPE_WEBHOOK_SECRET`; with the key alone control doesn't start.
- **Behind a proxy** that passes the client's address in a header, use
  `--trust-proxy-header` (for example `Fly-Client-IP`), so rate limits are
  per client (an IPv6 client counts by its /64).
- **Daemons from before 0.17** sign their requests to control the old
  way. Control takes that, each signature once, unless it's started with
  `--refuse-old-daemon-signatures` (`ARUGULA_CONTROL_REFUSE_OLD_DAEMON_SIGNATURES=1`),
  when those daemons are told to update. A machine control already knows
  joins again only from 0.17 on.
- **Build it** with `just static` (`target/x86_64-unknown-linux-musl/release/arugula-control`).
  - The web client is built into the binary.
  - `--static-dir web/dist` serves a local build instead.
- **The hosted one** is `packaging/control/` on Fly: `just control-deploy`.

- **Relay limits per account**, so one account can't take the whole
  machine (0 turns each off):
  - `ARUGULA_RELAY_MAX_SOCKETS` (32): client connections through the
    relay at once (a page uses one for all its machines).
  - `ARUGULA_RELAY_MAX_MACHINES` (50): machines dialed in at once.
  - `ARUGULA_RELAY_DAILY_MB` (2000): relayed traffic a day, while
    billing is off. Past it the account's relayed traffic slows to about
    64 KB/s, as billing's allowance does. Direct connections don't count.
- **A ceiling on the relay,** every account's sockets together:
  `ARUGULA_RELAY_MAX_TOTAL` (5000; 0 for none). Keep it below the
  proxy's connection limit, so a full relay still leaves room for
  control's pages and sign-ins. Past it a new relay socket is refused
  (what's open stays): daemons and the CLI get `503` with
  `Retry-After: 30`, and a browser's socket closes at once with code
  1013 and the reason. Daemons wait that long and up to as long again;
  a page says control is full and waits 30 to 60 seconds.
- **Backups:** set `LITESTREAM_BUCKET` and control backs its database up
  continuously with Litestream (below).

Daemons join a self-hosted control the same way:
`arugulad join https://control.example.com`. To have Getting started's
*Cloud* step join it too, start the daemon with
`--control https://control.example.com` (or `ARUGULA_CONTROL`).

## Operating the hosted one

The hosted control is one Fly machine (`packaging/control/fly.toml`):
`shared-cpu-1x` with 1 GB, in `ewr`, SQLite on the `control_data` volume.

### Capacity

An idle relay connection costs control about 28 KB: in a
1 GB VM, 3,900 of them took 121 MB. So the connection limits in
`fly.toml` (6,000 soft, 8,000 hard) are about open files, not memory.
Control raises its open-file limit to the hard one at start and logs it
(`open file limit open_files=…` in `fly logs`); keep `hard_limit` below
that. Each account is held to the relay limits above, and the relay as
a whole to its ceiling (5,000, under Fly's soft limit, so pages and
sign-ins still get through when it's full). Each minute it changed,
`fly logs` has `relay sockets` with the count (`sockets`, `daemons`,
`clients`), the ceiling (`max`) and how many were refused for it
(`refused`); `the relay is full` and `the relay has room again` mark
when it fills and empties.

### Backups (Litestream to Cloudflare R2)

Off until its secrets are set;
then:

- on start, with no database on the volume (a new or lost volume),
  control restores the latest backup first. If the bucket can't be
  reached it doesn't start, rather than start empty;
- it runs `litestream replicate` beside itself (in the image at
  `/litestream`): changes reach R2 within about a second. A snapshot a
  day, each kept 7 days, so a point-in-time restore reaches back a week,
  and anything deleted is gone from the backup within a week;
- on a stop it lets Litestream sync one last time.

To turn it on:

1. In Cloudflare: *R2 → Create bucket* (the hosted one's is `illogical-control-backups`,
   automatic location). Then *R2 → Manage API tokens → Create API token*:
   *Object Read & Write*, for that bucket only. Note the access key id,
   the secret, and the S3 endpoint
   (`https://<account id>.r2.cloudflarestorage.com`).
2. Set the secrets (Fly restarts the machine with them):

   ```
   fly secrets set -a illogical-control \
     LITESTREAM_BUCKET=illogical-control-backups \
     LITESTREAM_ENDPOINT=https://<account id>.r2.cloudflarestorage.com \
     LITESTREAM_ACCESS_KEY_ID=… LITESTREAM_SECRET_ACCESS_KEY=…
   ```

   `LITESTREAM_PATH` (default `control`) is the prefix in the bucket;
   `LITESTREAM_REGION` defaults to `auto`.
3. Check `fly logs`: `backing up with Litestream`, then Litestream's own
   `replicating to` and `snapshot complete` lines.

### Restoring

- **Onto a new volume:** create it (`fly volumes create control_data -r ewr
  -s 1`), point the machine at it (or destroy the old volume), and start
  the machine. Control finds no database and restores the latest backup.
- **To look at a backup, or go back in time,** on any machine with
  `litestream` and the same four values in its environment:

  ```
  cat > ls.yml <<EOF
  dbs:
    - path: /tmp/control.db
      replica: {type: s3, bucket: illogical-control-backups, path: control, region: auto, endpoint: "https://<account id>.r2.cloudflarestorage.com", force-path-style: true}
  EOF
  LITESTREAM_ACCESS_KEY_ID=… LITESTREAM_SECRET_ACCESS_KEY=… litestream restore -config ls.yml -o /tmp/control.db /tmp/control.db
  ```

  Add `-timestamp 2026-10-04T12:00:00Z` for a point in time. To put it
  back: stop the machine, copy the file over `/data/control.db` (and
  delete `control.db-wal`, `control.db-shm` and `.control.db-litestream`
  beside it), start it.
- **Restored once to prove it** (2026-10-04, staging): a control in one VM
  replicated to an S3-compatible store in another; the VM was destroyed;
  a fresh VM with an empty disk restored on start, and every account,
  device and machine was there, and replication carried on.

Fly's own daily volume snapshots (kept 5 days) still run.

### Product analytics (PostHog)

Control sends product events to PostHog when it has a project API key
(`crates/control/src/analytics.rs`). Without one, which is how every
self-hosted control starts, it sends nothing:

```
fly secrets set -a illogical-control POSTHOG_API_KEY=phc_…
# POSTHOG_HOST defaults to https://us.i.posthog.com; EU projects:
#   POSTHOG_HOST=https://eu.i.posthog.com
```

`fly logs` says `product analytics on` at start, or `no POSTHOG_API_KEY`.
Events are batched (every 10 s or 100 events) off the request path. A
full queue (10,000) drops them, and whatever is queued is sent when
control stops.

What's in them:

- **Who**: the account id as `distinct_id`. No name, login, email or IP
  (`$geoip_disable`). A machine's join request, which comes before any
  account, goes on `device:<id>` with no person made for it. Team events
  carry a `team` group (`$groups`), whose properties are its member and
  owner counts and whether it's locked.
- **Never**: terminal bytes (control can't read them), prompts, repository
  URLs, secret names or values, push endpoints, invite keys or join codes.
- **Requests**: `control request` for every request a person (a session or
  the CLI) makes that changes something, and every one that fails, with
  its route template (`/api/devices/{id}/approve`, never the real path),
  method, status and time taken. Reads that succeed are left out (pages
  poll them), and so are daemons' own requests.
- **Sign-in**: `signed up`, `signed in` (`method`: github, passkey, app),
  `sign-in failed`, `signed out`, `session ended`, `passkey added`,
  `passkey removed`, `desktop app sign-in allowed`, `name changed`,
  `account deleted`.
- **Devices and machines**: `device enrolled`, `device requested`,
  `device approved`, `device turned down`, `device removed`,
  `recovery codes made`, `machine join started`, `machine joined`,
  `machine join turned down`, `cli joined`, `machine moved`,
  `machine left`, `share answered`.
- **Teams**: `team created`, `team roster changed`, `team joined`,
  `team invite made`, `team invite cancelled`, `team join requested`,
  `team request turned down`, `team locked`, `team unlocked`.
- **Relay**: `relay session` (kind, seconds, bytes each way, slowed),
  `machine relay session` (seconds a machine stayed dialed in),
  `relay refused` (account limit or relay full).
- **Paid things**: `checkout started`, `upgraded`, `subscription changed`,
  `sandbox created`, `sandbox failed`, `sandbox deleted` (minutes),
  `cloud agent created/updated/deleted/imported`, `cloud run started`,
  `cloud run finished` (outcome, minutes), `cloud run cancelled`,
  `cloud request answered`, `cloud secret saved/deleted`.
- **Other**: `push turned on` (the push service's host), `push turned off`,
  `push relayed` (status), `labs flag set`.
- **Machines' versions**: `machine version` (machine, version, os, arch)
  when a joined daemon's trust request (`GET /api/daemon/trust?…&version=&os=&arch=`)
  says something other than last time. Control keeps them on the daemon's
  row. This needs no opt-in: it describes a machine joined to control, as
  its name and addresses do. Older daemons say nothing.
- **Installs' telemetry** (`POST /api/telemetry`, opt-in on the machine,
  `crates/control/src/telemetry.rs`): a report signed by a joined daemon
  goes on its account, with `machine` (the daemon id); an unsigned one
  goes on `install:<id>` with no person made for it. Each carries
  `install`, `version`, `os`, `arch`, `channel` and `control`:
  `install opted in` (via), `install started` (service), `install day`
  (`panes_opened`, `panes_peak`, `agents_started_<runtime>`,
  `blocks_opened_<kind>`, `clients_<surface>`, `view_<view>`, `flags_on`
  as a list, `web_errors`), and `crash` (process, `version` that crashed,
  `install_version` reporting it, location, frames, at). A report is
  checked (`Report::check`, at most 64 KB) and refused with 400 or 413;
  unsigned ones are limited to 600 an hour per client address, and 30 an
  hour per install (or per daemon, signed), past which they get 429.
  Without `POSTHOG_API_KEY` control takes reports and forwards nothing.
- **Control's page's usage** (M104, opt-in on the account): the page isn't
  any one machine's, so its yes is the account's, kept on its row
  (`accounts.telemetry`: NULL not asked, 0 no, 1 yes; `telemetry_at` when).
  `GET /api/me/telemetry` says `{on, at, ask}` (`ask` is false without
  `POSTHOG_API_KEY`, and then the page never asks); `POST {on}` answers,
  from the card the page shows once after sign-in or the switch under
  Sign-in and account. A yes sends `page opted in`; a no sends nothing.
  With yes, the page sends what it counted each minute and as it closes
  to `POST /api/telemetry/page {views, web_errors}` (a session, same
  origin), sent as `page day` under the account: `view_<view>` for each
  view opened (`view_other` for one control doesn't know), `web_errors`
  (a count; never a message or a stack). Without yes it's refused (403,
  reason `telemetry_off`) whatever a page sends; it's limited to 300 an
  hour per account (429). Neither route makes a `control request`.
  Control's own events above don't depend on this answer.

Deleting an account sends `account deleted`, but doesn't delete the
person in PostHog: do that in PostHog (or by its API) when someone asks.

### A spend alert

A spend alert isn't something this repository sets: it belongs to
the Fly organization's billing settings in Fly's dashboard (it's the
operator's to set).

## Testing

- `just control-smoke` runs it end to end without a browser:
  - a fake GitHub sign-in, then enrolment and approval;
  - a daemon joining by code;
  - its API and protocol through the relay and directly;
  - a check that control's wire traffic, database and logs never contain
    what was typed;
  - deleting an account: its machine is refused, and no row in the
    database mentions it.
- `web/e2e/control.spec.ts` and `web/e2e/passkey.spec.ts` drive it in Chrome.
- `web/e2e/device-keys.webkit.spec.ts` drives it in Playwright's WebKit
  (`pnpm exec playwright install webkit` first): device keys survive a
  reload, a join is approved from a later page load, and a browser that
  lost its key enrolls again.
- **Safari itself:** open `/key-probe.html` on control (or any daemon) in
  Safari. It stores a set of device keys, reloads, and says whether they
  still work, and which Safari it is.
- Control logs every refused enrolment and approval (`refused`, with the
  request, the reason, and the account, device and approver ids; never a
  signature or a body), so `fly logs` shows why.
