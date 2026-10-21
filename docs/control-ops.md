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
- **Shared Docs** (#1064): always on, and idle until a machine calls. With
  a block domain, anyone opens a link at `docs.<domain>` (a fixed label,
  so the wildcard DNS name and certificate cover it); without one, at
  `/d/<id>` on control's own origin. Either way it's a page under a strict
  CSP, with no cookie and none of control's API. The `docs` host answers
  only `/<id>`, `/<id>/data/meta`, `/<id>/data/<n>` and `/assets/…`; with a
  block domain, `/d/` on control's origin is a 404. `/control.json` says
  where (`doc_links`, `doc_viewer`). More under "Shared Docs" below.
- **Billing** (optional) needs both `STRIPE_SECRET_KEY` and
  `STRIPE_WEBHOOK_SECRET`; with the key alone control doesn't start.
- **Behind a proxy** that passes the client's address in a header, use
  `--trust-proxy-header` (for example `Fly-Client-IP`), so rate limits are
  per client (an IPv6 client counts by its /64).
- **Daemons from before 0.17** signed their requests to control the old
  way and joined without their key's proof. Control refuses both (426) and
  tells them to update.
- **Build it** with `just static` (`target/x86_64-unknown-linux-musl/release/arugula-control`).
  - The web client is built into the binary.
  - `--static-dir web/dist` serves a local build instead.
- **The hosted one** is `packaging/control/` on Fly. Each release tag
  deploys it ("Deploys" below); `just control-deploy` deploys from a
  laptop.

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
Its platform (chat, Cloud, billing, projects and grants) runs inside it
until M141 moves it to its own app, `arugula-platform` ("The platform"
below).

### The volume

`control_data` is 10 GB, mounted at `/data`. It holds `control.db` with
its WAL, Litestream's working files, and Shared Docs' ciphertext, which
can reach 200 MB per account (below). A volume can grow but never
shrink, so it was made 10 GB once rather than grown a step at a time.
It was made at 1 GB and extended on 2026-10-20 (M132) with:

```
fly volumes list -a illogical-control      # the id: vol_r68lmz2doox9myd4
fly volumes extend vol_r68lmz2doox9myd4 -s 10 -a illogical-control
```

The restore recipes below need a volume at least as big as the
database.

### Secrets

What `illogical-control` has set with `fly secrets set`, and what reads
each. Anything else in `fly secrets list` is stale: record its value in
Infisical (project `arugula`, env `prod`), then `fly secrets unset` it.
The rest of control's settings are in `fly.toml`'s `[env]`.

- `GITHUB_APP_ID`, `GITHUB_APP_SLUG`, `GITHUB_APP_CLIENT_ID`,
  `GITHUB_APP_CLIENT_SECRET`, `GITHUB_APP_WEBHOOK_SECRET`,
  `GITHUB_APP_PRIVATE_KEY`: the GitHub App (`main.rs`). Its client id and
  secret also sign people in, since `GITHUB_CLIENT_ID` and
  `GITHUB_CLIENT_SECRET` aren't set.
- `LITESTREAM_BUCKET`, `LITESTREAM_ENDPOINT` (`backup.rs`), and
  `LITESTREAM_ACCESS_KEY_ID`, `LITESTREAM_SECRET_ACCESS_KEY` (checked by
  `backup.rs`, read by Litestream from the environment it inherits).
- `ARUGULA_MANAGOAT_API_KEY`: Cloud workspaces' provider. Without it,
  Cloud workspaces are off.
- `ARUGULA_CLOUD_SECRET_KEY`: encrypts cloud secrets at rest.
- `ARUGULA_CLOUD_ACCOUNTS`: who may use Cloud workspaces (`*` for all).
- `ARUGULA_CLOUD_MODELS`: the `runtime:model` pairs a Cloud workspace's
  tabs may run on, the first the default. `ARUGULA_CLOUD_MODEL` and
  `ARUGULA_CLOUD_RUNTIME` are only read when it's unset
  (`cloud::configured_models`), so with it set they're stale.
- `POSTHOG_API_KEY`: product analytics (`analytics.rs`).
- `ARUGULA_PLATFORM_KEY`: the key the hub and an out-of-process platform
  share ("The platform" below). Unused while the platform runs in process.
- `SPRITES_TOKEN`: hosted sandboxes (M20). Only accounts in
  `ARUGULA_SANDBOX_ACCOUNTS` may make one, and that isn't set, so they're
  off for everyone.

Read, but they do nothing: `ARUGULA_CLOUD_DAILY_RUNS`,
`ARUGULA_CLOUD_CONCURRENT_RUNS`, `ARUGULA_CLOUD_RUN_MINUTES`. Cloud runs
were retired; control still parses them so an old deploy starts, and
logs that they do nothing.

Unset on 2026-10-20 (M132), their values kept in Infisical under
`/illogical/control`: `ILLOGICAL_SANDBOX_ACCOUNTS`, which nothing read (the
code reads `ARUGULA_SANDBOX_ACCOUNTS`, so renaming it would have turned
hosted sandboxes on for the accounts it listed), and `ARUGULA_CLOUD_MODEL`
and `ARUGULA_CLOUD_RUNTIME`, stale with `ARUGULA_CLOUD_MODELS` set.

### Daemon census

Control keeps, on each daemon's row, when it was last on the relay
(`last_seen`) and the version it last said it runs (`version`, from
`GET /api/daemon/trust`; daemons before 0.43.0 never say).
`arugula-control census` counts the daemons seen in the last 30 days
(`--days`) by version, with the oldest and newest last-seen for each. It
opens the database read-only, so it runs beside the live control:

```
just control-census
```

which is `fly ssh console -a illogical-control -C "/arugula-control census --db /data/control.db"`.
It sizes the version skew a change to control has to survive. The
volume no longer carries a `sqlite3` binary (removed 2026-10-20 with the
other leftovers from 2026-10-03): for another read-only query, fetch a
copy with `fly ssh sftp get /data/control.db` or restore one from
Litestream (below), and query it locally.

### Deploys

Each release tag deploys control (M133, #1129). It's automatic, with no
approval step.

- **The `control` job** in `.github/workflows/release.yml` runs as soon
  as the `linux` job has built the tag's static binaries. It doesn't wait
  for macOS, Windows or the release to be published.
- **The image** is built on GitHub's runner the way `just control-deploy`
  builds it: `packaging/control/Dockerfile` around `arugula-control` and
  `arugulad`, with `fly deploy --local-only`. It's labelled with the tag,
  so `fly releases -a illogical-control` shows
  `registry.fly.io/illogical-control:vX.Y.Z`.
- **After the deploy,** `scripts/ci-control-deploy` checks that
  `https://control.arugula.io/control.json` answers and that every machine
  runs the tag's image with its health check passing. If either fails,
  the job fails. To run the same check by hand:
  `scripts/ci-control-deploy check vX.Y.Z`.
- **One deploy at a time.** A tag's deploy waits for the one before.
  With a third tag, the one waiting is dropped, since the newer tag
  deploys anyway.
- **Re-running an older tag's job** doesn't take control back: if a newer
  release is already running, the job says so and deploys nothing.
- **The token** is the repository secret `FLY_API_TOKEN`, a deploy token
  for this app only. Without it the job fails and says so; control isn't
  deployed until it's set and the job is re-run. The one set on
  2026-10-20 is named `github-release-arugula-v2` and expires in 2046
  (`fly tokens list -a illogical-control`). To replace it, make a new one
  (`-x` sets a shorter expiry), then revoke the old:

  ```
  fly tokens create deploy -a illogical-control -n github-release -x 8760h
  gh secret set FLY_API_TOKEN -R arugula-salad/arugula
  ```

- **Hotfixes:** `just control-deploy` builds and deploys from your
  checkout. Its image is labelled `deployment-…`, not a version. The next
  tag deploys over it. Don't run it while a tag's `control` job is
  deploying: the two don't wait for each other.
- **Rolling back:** since a newer release stays (above), re-running an
  older tag's job won't do it. Deploy the older tag's image instead, from
  `packaging/control/`:
  `fly deploy --image registry.fly.io/illogical-control:vX.Y.Z`.

### The platform

Control is two processes ("Control is two processes: a hub and a
platform", #1150): the hub (`arugula-control`: identity, devices, rosters,
the relay, push, block and doc origins, the jump host, forge, telemetry)
and the platform (`arugula-platform`: chat, Cloud, billing, projects and
grants). The platform runs in one of two ways, from the same binaries:

- **In process** (`ARUGULA_PLATFORM=in-process`, the default, and the
  rollback): inside the hub, as one Fly app. Production runs this way
  until M141's window.
- **Out of process** (`ARUGULA_PLATFORM=http://arugula-platform.internal:7700`):
  its own Fly app, `arugula-platform` (`packaging/platform/fly.toml`), with
  its own volume (`platform_data` at `/data`, `platform.db`). The hub is
  its gateway: every route `crates/control/routes.toml` gives
  `process = "platform"` stays on `control.arugula.io` ("Platform paths
  stay on control.arugula.io forever"), and the hub forwards it (M140,
  `crates/control/src/gateway.rs`). A platform deploy or crash restarts no
  relay socket.

**Two apps.**

| | `illogical-control` (hub) | `arugula-platform` |
|---|---|---|
| Image | `packaging/control/Dockerfile` | `packaging/platform/Dockerfile` |
| Public | `control.arugula.io` (`[http_service]`, 7690) | nothing: no service, no public IP |
| Private | its internal API, `[::]:7691` (`HubPort` for the platform) | `[::]:7700` (the forwarded routes, and `PlatformPort` for the hub) |
| Volume | `control_data` (10 GB) | `platform_data` |
| Health | `/control.json` through the proxy | `/platform.json`, a machine check |
| Logs | `fly logs -a illogical-control` | `fly logs -a arugula-platform` |

**How they reach each other.** Over Fly's private network (6PN, carried
over WireGuard between Fly's hosts), by `.internal` names: the hub calls
`http://arugula-platform.internal:7700`, the platform calls
`http://illogical-control.internal:7691`. Not Flycast: Flycast needs a
private IP and a service in `fly.toml`, and goes through Fly's proxy, an
extra hop with nothing to balance across one machine. M144 (two platform
machines) revisits it.
- The hub's internal API listens on its own port, which no `fly.toml`
  service names, so Fly's public proxy never reaches it. It also refuses
  anything carrying `Fly-Forwarded-Port` or `Fly-Client-IP` (the headers
  that proxy adds), in case someone ever declares a service for it, and
  every call has to be signed with the shared key.
- The platform has no service and no public IP. Never
  `fly ips allocate-v4/v6` for it.

**The shared key.** `ARUGULA_PLATFORM_KEY`, a Fly secret on both apps,
32 random bytes as hex (`openssl rand -hex 32`); keep it in Infisical
(`arugula` prod, `/illogical/control`). It's used two ways, never sent:
- the hub signs who's asking into each forwarded call's
  `x-arugula-principal` header (HMAC-SHA256 over the principal, method,
  path and query, time and the request id; ±60 s), and the platform
  answers nothing that lacks a valid one;
- each internal call, either way, is signed in `x-arugula-internal`
  (HMAC-SHA256 over which process is called, the method, path, time, a
  nonce and the body's hash; each nonce taken once).

HMAC rather than a bearer token: the key never crosses the network or
lands in a log, and a captured call is good once, for that one request.
To rotate it, set the new one on both apps (`fly secrets set` restarts
each): calls fail with 401 between the two restarts, so do it in a
maintenance window.

**What the gateway does** with each platform route: answers 503 while the
maintenance switch is on; checks the session (and its same-origin rule)
or the device's or daemon's signature exactly as the hub always has,
replay cache included; drops the cookie, the signature and any
principal or internal header a client sent; forwards the call with the
signed principal header, the method, path, query, body and Host header
untouched (a daemon's signature covers the body); and passes the status,
headers (`Retry-After` among them) and body back as they come, streamed,
so chat's live stream (SSE, with `Last-Event-ID`) goes through
unbuffered. A body up to 1 MB is read first; a bigger one streams. If
the platform can't be reached at all, the call is tried again for 5
seconds, then answered with the maintenance 503 (`Retry-After: 5`): it
never reached the platform, so clients send it again. `/api/directory`'s
"chat is on" comes from the hub's own Labs flags, so the directory never
waits on the platform.

**Logs.** The hub logs each forwarded call that fails with a request id
(`the platform can't be reached`, `the platform didn't answer`); the
platform logs each call it handles in a `call{request=…}` span with the
same id, so `fly logs -a arugula-platform | grep <id>` finds the other
half. The platform also logs refused calls (`refused a call the hub
didn't vouch for`, `refused an internal call`): a key that differs between
the two apps shows up there as 401s.

**Deleting an account** out of process is two steps: the hub commits,
then tells the platform (`PlatformPort::account_deleted`). Until the
platform confirms, a row in the hub's `untold_deletions` keeps what to
tell it, and the hub tries again every 30 seconds.

**Deploys.** A release tag deploys the platform first (release.yml's
`platform` job), then the hub (`control`, which waits for it but deploys
whatever became of it). `scripts/ci-platform-deploy` builds the platform's
image on GitHub's runner and checks every machine runs the tag with its
health check passing. It needs the repository secret
`FLY_PLATFORM_API_TOKEN`, a deploy token for `arugula-platform`, which
can't exist before the app does: without it the job says so and
succeeds, deploying nothing, and the hub deploys as before. Hotfix:
`just platform-deploy`. Roll back: from `packaging/platform/`,
`fly deploy --image registry.fly.io/arugula-platform:vX.Y.Z`. An older
platform starts on a database whose migrations it doesn't know only while
all of them are additive (M142, "Checking a migration on a copy" below);
past one that isn't, restore `platform.db` too. A platform
deploy restarts no relay socket; pages' chat streams reconnect and
resync.

**Making the app** (once, before M141). It lives in `iad`, beside its
Managed Postgres, which Fly doesn't offer in `ewr`; the hub stays in `ewr`,
about 5 ms away:

```
fly apps create arugula-platform -o <org>
fly volumes create platform_data -a arugula-platform -r iad -s 1
KEY=$(openssl rand -hex 32)            # keep it in Infisical too
fly secrets set -a arugula-platform --stage ARUGULA_PLATFORM_KEY=$KEY ARUGULA_PLATFORM_PAUSED=1
fly secrets set -a illogical-control --stage ARUGULA_PLATFORM_KEY=$KEY
fly tokens create deploy -a arugula-platform -n github-release -x 8760h
gh secret set FLY_PLATFORM_API_TOKEN -R arugula-salad/arugula
```

`--stage` sets a secret without restarting anything; the hub only reads
`ARUGULA_PLATFORM_KEY` once it runs the platform out of process. The
platform's cloud and Litestream secrets are set from the hub's own values
and Infisical, without printing them, the day before the window ("The
platform's secrets" in its runbook): `ARUGULA_CLOUD_SECRET_KEY` above
all, which decrypts every stored cloud secret. The next tag deploys the platform; until M141 it runs with an
empty database beside the hub, which still runs its own platform in
process and answers every platform route itself.

**Flipping to out of process** is M141's window ("The first window"
below): the maintenance switch on with `--pause`, the platform's tables
copied from `control.db` to `platform.db` and proven, then
`fly secrets set -a illogical-control ARUGULA_PLATFORM=http://arugula-platform.internal:7700`.
That restarts the hub once (relay sockets reconnect on their own), with
its internal API on `[::]:7691`.

**Rolling back to in process:** hold the platform first
(`ARUGULA_PLATFORM_PAUSED=1`), so two cloud workers never run on two
files, then `fly secrets unset -a illogical-control ARUGULA_PLATFORM` (or
set it to `in-process`): the hub restarts running its own platform on
`control.db`, migrating its platform tables as the hub's build knows them
(the same additive-only rule). After M141 that file's platform tables are
the 14-day read-only copies, so anything written on the platform since is
in `platform.db`: copy it back first ("Rolling back", in the window's
runbook).

**The maintenance switch** (M141): every platform route answers `503`,
`Retry-After: N`, `{"error": "Chat and Cloud are down for maintenance; try again in N seconds.", "reason": "maintenance"}`
before anything is forwarded; terminals, the relay and every hub route
are untouched. It's a row in the hub's database, which the running hub
reads every second, so setting it restarts nothing and it holds across a
restart:

```
fly ssh console -a illogical-control -C "/arugula-control maintenance on --retry-after 30 --pause --db /data/control.db"
fly ssh console -a illogical-control -C "/arugula-control maintenance status --db /data/control.db"
fly ssh console -a illogical-control -C "/arugula-control maintenance off --db /data/control.db"
```

`--pause` also holds the in-process platform's background work (the
cloud worker, approval pushes, usage reports, the projects and grants
feed follower), so its tables hold still while they're copied. An
out-of-process platform is held with `ARUGULA_PLATFORM_PAUSED=1` on its
own app (`fly secrets set -a arugula-platform ARUGULA_PLATFORM_PAUSED=1`,
a platform restart, which drops nothing); unset it to resume.

### The first window (M141)

The platform's tables move from the hub's `control.db` to the platform's
own `platform.db`, and the hub starts forwarding to the out-of-process
platform (#1158). Chat and Cloud answer the maintenance 503 for the few
minutes it takes; terminals, the relay and sign-in don't notice. Rehearsed
end to end, both rollbacks too, with `testnet/window.sh`
([testnet/README.md](../testnet/README.md)).

**The tools.** Both binaries carry the platform's own copy code
(`crates/platform/src/window.rs`), because the two apps have separate
volumes and the file has to travel between them, through your laptop,
with `fly ssh sftp`:

- `copy --from F --to T [--replace]` copies every table the platform's
  migrations make (`process = "platform"` in `tables.toml`, but
  `platform_migrations`, which the target's own runner writes when it
  opens it), rowids kept, in one transaction. It rebuilds `chat_fts` from
  the copied messages and runs FTS5's `integrity-check`. Then it counts
  and checksums every table on both sides (SHA-256 over each row's rowid
  and values in column-name order, the row digests sorted) and commits
  only if every one matches: otherwise nothing is written. It refuses a
  target that already holds rows, unless `--replace`, which empties those
  tables first in the same transaction. The source is opened read-only,
  so it can be the live `control.db`.
- `checksums --db F` prints the same counts and checksums, read-only.
- `secrets-check --db F` opens every cloud secret in F with the running
  server's `ARUGULA_CLOUD_SECRET_KEY` and says how many open. It prints no
  value, and names the server's pid.

On the hub they're `/arugula-control platform copy|checksums|secrets-check`,
on the platform `/arugula-platform copy|checksums|secrets-check`.

**What has to be true first:**
- A release with M140 (the gateway, the internal API and the switch) and
  this runbook's tools is deployed to both apps; the platform runs it,
  held, on an empty `platform.db`. Before this release, the platform
  refused `ARUGULA_PLATFORM_PAUSED=1` (only `true` was taken) and didn't
  start.
- `ARUGULA_CLOUD_SECRET_KEY` is escrowed in Infisical (`arugula` prod,
  `/illogical/control`), round trip checked. Done 2026-10-20.
- **The VAPID key stays in the hub's database. Never regenerate it**: that
  kills every push subscription.

Keep two terminals on the logs through all of it:
`fly logs -a illogical-control` and `fly logs -a arugula-platform`. The
commands below use two helpers and a scratch directory; paste these first:

```
hub()  { fly ssh console -q -a illogical-control -C "/arugula-control $*" | tr -d '\r'; }
plat() { fly ssh console -q -a arugula-platform -C "/arugula-platform $*" | tr -d '\r'; }
copied()  { awk 'NF == 6 && $NF == "ok" { print $1, $5 }' "$1" | sort; }   # a copy's checksums
current() { awk 'NF == 3 && length($3) == 64 { print $1, substr($3, 1, 16) }' "$1" | sort; }   # checksums'
mkdir -p ~/m141 && cd ~/m141 && umask 077
```

#### The platform's secrets (a day ahead, staged)

What `arugula-platform` needs besides `ARUGULA_PLATFORM_KEY` and
`ARUGULA_PLATFORM_PAUSED=1` (both staged on 2026-10-20). `--stage`
restarts nothing; the platform's next restart (the window's unpause)
takes them.

| Secret | From |
|---|---|
| `ARUGULA_CLOUD_SECRET_KEY` | Infisical (Fly can't read a secret back) |
| `ARUGULA_MANAGOAT_API_KEY`, `ARUGULA_CLOUD_ACCOUNTS`, `ARUGULA_CLOUD_MODELS`, `POSTHOG_API_KEY` | the hub's running process |
| `LITESTREAM_BUCKET`, `LITESTREAM_ENDPOINT`, `LITESTREAM_ACCESS_KEY_ID`, `LITESTREAM_SECRET_ACCESS_KEY` | the hub's (the same bucket; the platform writes under `platform`, the hub under `control`), or a bucket and token of its own (below) |

Not copied: `ARUGULA_CLOUD_DAILY_RUNS`, `ARUGULA_CLOUD_CONCURRENT_RUNS` and
`ARUGULA_CLOUD_RUN_MINUTES` (the run caps were retired; the platform would
only log that they do nothing), the Stripe pair (billing is off), and
everything that's the hub's own (`GITHUB_APP_*`, `SPRITES_TOKEN`).
`LITESTREAM_PATH` stays unset (it defaults to `platform`; the platform
refuses `control`, the hub's).

The cloud key, from Infisical (run where arugula's `.infisical.json` is, or
add `--projectId`), printing nothing:

```
printf 'ARUGULA_CLOUD_SECRET_KEY=%s\n' "$(infisical secrets get ARUGULA_CLOUD_SECRET_KEY --env prod --path /illogical/control --plain --silent)" \
  | fly secrets import --stage -a arugula-platform
```

The rest from the hub's running process, whose pid `secrets-check` names
("the running server's (pid N)"): fetch its environment, keep the names
wanted, and pipe them into Fly. Python prints only what's missing:

```
hub platform secrets-check --db /data/control.db        # note N; every secret opens
fly ssh sftp get /proc/N/environ hub.environ -a illogical-control
python3 -I - hub.environ <<'PY' | fly secrets import --stage -a arugula-platform
import sys
want = ["ARUGULA_MANAGOAT_API_KEY", "ARUGULA_CLOUD_ACCOUNTS", "ARUGULA_CLOUD_MODELS", "POSTHOG_API_KEY",
        "LITESTREAM_BUCKET", "LITESTREAM_ENDPOINT", "LITESTREAM_ACCESS_KEY_ID", "LITESTREAM_SECRET_ACCESS_KEY"]
raw = open(sys.argv[1], "rb").read()
if not raw:
    sys.exit("hub.environ is empty: sftp read /proc as 0 bytes. Don't guess values; stop and ask.")
env = dict(kv.split("=", 1) for kv in raw.decode().split("\0") if "=" in kv)
missing = [k for k in want if not env.get(k)]
if missing:
    sys.exit("not in the hub's environment: " + ", ".join(missing))
for k in want:
    print(f"{k}={env[k]}")
PY
shred -u hub.environ
fly secrets list -a arugula-platform     # names and digests only
```

For a bucket of its own instead (Cloudflare: *R2 → Create bucket*
`arugula-platform-backups`, then an *Object Read & Write* token for that
bucket only), drop the four `LITESTREAM_*` names from `want` and set them
from Cloudflare with `fly secrets set --stage -a arugula-platform …`.

#### The notice (a day ahead)

Control's notice (#206) is a row per account that the page shows once in a
dialog until it's dismissed. `notice post` writes one to every account
there is now; `--from` takes a file (first line the title, the rest the
body), so nothing has to get through `fly ssh console -C`'s quoting.
`/tmp` is cleared by the next restart.

```
cat > notice.txt <<'EOF'
Chat and Cloud maintenance: <Day> <HH:MM> UTC
Chat and Cloud will be down for about 15 minutes from <HH:MM> UTC on <Day>, while they move to a server of their own. Terminals, the relay and signing in keep working, and agents' calls wait it out. Afterwards, reload any control page you have open: pages opened before then don't reconnect chat by themselves.
EOF
fly ssh sftp put notice.txt /tmp/notice.txt -a illogical-control
hub notice post --db /data/control.db --from /tmp/notice.txt --dry-run
hub notice post --db /data/control.db --from /tmp/notice.txt
hub notice list --db /data/control.db
```

Moved? `hub notice withdraw --db /data/control.db --from /tmp/notice.txt`
takes back the ones nobody has dismissed (the same title), then post the
new time.

#### Pre-checks (an hour before)

```
fly status -a illogical-control          # every machine on the release's image, checks passing
fly status -a arugula-platform           # the same; started
fly logs -a arugula-platform --no-tail | grep -E 'paused|backing up with Litestream|replicating to'
curl -fsS https://control.arugula.io/control.json
hub maintenance status --db /data/control.db    # off
hub platform checksums --db /data/control.db | tee before.txt
# A trial copy, carried as the window will, timed: its secrets must open with
# the platform's key (the one from Infisical).
time hub platform copy --from /data/control.db --to /tmp/trial.db | tee trial.txt
time fly ssh sftp get /tmp/trial.db trial.db -a illogical-control
time fly ssh sftp put trial.db /tmp/trial.db -a arugula-platform
plat secrets-check --db /tmp/trial.db    # "N open", the same N as the hub's
shred -u trial.db
```

The trial's timings are the window's copy and carry. Also take a baseline
of proxied latency, from the page's cookie (devtools → Application →
Cookies), for after:

```
COOKIE='session=…'
for i in $(seq 300); do curl -s -o /dev/null -w '%{time_total}\n' -H "cookie: $COOKIE" \
  -H 'origin: https://control.arugula.io' https://control.arugula.io/api/chat; done \
  | sort -n | awk '{ a[NR] = $1 } END { printf "p50 %.0f ms  p95 %.0f ms\n", a[int(NR/2)] * 1000, a[int(NR*0.95)] * 1000 }'
```

#### The window

1. **Maintenance on, the in-process platform held.** Platform routes answer
   503 within a second.

   ```
   hub maintenance on --retry-after 60 --pause --db /data/control.db
   curl -si https://control.arugula.io/api/chat | head -4     # 503, Retry-After: 60
   curl -fsS https://control.arugula.io/control.json          # the hub answers
   ```

2. **Let the worker drain.** A tick in flight finishes its call to
   Managoat (each times out in 15 s). `sleep 30`. Step 6 proves nothing
   moved after the copy.

3. **Copy on the hub**, from the live file (read-only) to `/tmp`:

   ```
   hub platform copy --from /data/control.db --to /tmp/window.db | tee window-hub.txt
   ```

   It ends `22 tables, N rows: every count and checksum matches`.

4. **Carry it:**

   ```
   fly ssh sftp get /tmp/window.db window.db -a illogical-control
   fly ssh sftp put window.db /tmp/window.db -a arugula-platform
   ```

5. **Copy on the platform** into the running (held) platform's own file:

   ```
   plat copy --from /tmp/window.db --to /data/platform.db | tee window-platform.txt
   diff <(copied window-hub.txt) <(copied window-platform.txt) && echo same
   ```

6. **Nothing changed on the hub since**, and the platform's key opens every
   secret:

   ```
   hub platform checksums --db /data/control.db | tee hub-now.txt
   diff <(copied window-hub.txt) <(current hub-now.txt) && echo unchanged
   plat secrets-check --db /data/platform.db
   shred -u window.db
   ```

   If the hub's tables changed (a write slipped in: an account deleted, a
   tick that finished late), go back to step 3, and pass `--replace` in
   step 5.

7. **Flip.** One hub restart; relay sockets reconnect by themselves, and
   the switch holds across it:

   ```
   fly secrets set -a illogical-control ARUGULA_PLATFORM=http://arugula-platform.internal:7700
   hub maintenance status --db /data/control.db               # still on
   fly logs -a illogical-control --no-tail | grep -E 'runs out of process|internal API'
   ```

8. **Let the platform go.** One platform restart: it finds the hub's
   internal API at once, and its worker resumes Managoat's events from
   their cursor:

   ```
   fly secrets unset -a arugula-platform ARUGULA_PLATFORM_PAUSED
   fly logs -a arugula-platform --no-tail | grep -E 'Arugula platform|replicating to|refused'
   ```

   It shouldn't say `paused` or `refused`. (Flip first, then unpause, so
   the platform's first unheld start already reaches the hub, and a flip
   that goes wrong is undone with nothing written to `platform.db`.)

9. **Maintenance off:**

   ```
   hub maintenance off --db /data/control.db
   curl -si https://control.arugula.io/api/chat | head -1     # 401: answered, by the platform
   ```

#### Post-checks

- **Chat:** reload control's page. Chat lists your rooms; a room's
  messages are there; search finds an old message; post one.
- **A cloud secret written before the window decrypts after it:** step 6's
  `secrets-check` opened every one with the platform's key. In use: open a
  Cloud workspace whose spec names a secret and **Rebuild** it (the
  platform decrypts the secret into Managoat's vault), or make a new one
  that names it.
- **A Cloud workspace turn:** in a Cloud workspace's tab, send a prompt
  and see the answer.
- **Push still reaches a phone:** from a second account in a team
  channel, `@`-mention someone whose phone has control's page closed. The
  push goes platform → `HubPort` → the hub's VAPID key → the phone.
- **Agents:** `arugula` MCP's `cloud` tool lists workspaces (daemons with
  #1166 waited the window out, up to 5 minutes; older ones failed fast
  with "try again in 60 seconds").
- **Logs:** nothing `the platform can't be reached`, `refused a call the
  hub didn't vouch for` or `refused an internal call`.
- **Backups:** `fly logs -a arugula-platform` shows Litestream's
  `snapshot complete`.
- **Latency:** the curl loop above again. #1150's gate is p95 within 20 ms
  of the baseline; cold chat lists pay 5 to 7 hub calls over 6PN.
- Post the window's end in the notice's place if you like; the notice
  already told people to reload.

#### Rolling back

At each point, from where it went wrong:

- **Before step 7** (nothing flipped): `hub maintenance off --db /data/control.db`.
  The copy only read `control.db`; the hub still runs in process on it. The
  platform is still held; its file has a copy in it, so the next attempt
  passes `--replace` in step 5.
- **Step 7 went wrong** (the hub doesn't come back, or forwards errors):
  ```
  fly secrets unset -a illogical-control ARUGULA_PLATFORM     # in process again, on control.db
  hub maintenance off --db /data/control.db
  ```
  The platform hasn't been let go, so nothing was written anywhere.
- **After step 8, before step 9** (the platform ran, nobody wrote): hold it,
  then flip back. Its background writes (feed positions, Managoat events)
  are lost, and redone from older cursors by the hub:
  ```
  fly secrets set -a arugula-platform ARUGULA_PLATFORM_PAUSED=1
  fly secrets unset -a illogical-control ARUGULA_PLATFORM
  hub maintenance off --db /data/control.db
  ```
- **After step 9** (people wrote chat and Cloud on the platform): copy
  `platform.db` back over the hub's copies first. The hub doesn't touch
  those tables while it forwards, so the copy back runs beside it:
  ```
  hub maintenance on --retry-after 60 --pause --db /data/control.db
  fly secrets set -a arugula-platform ARUGULA_PLATFORM_PAUSED=1
  plat copy --from /data/platform.db --to /tmp/back.db | tee back-platform.txt
  fly ssh sftp get /tmp/back.db back.db -a arugula-platform
  fly ssh sftp put back.db /tmp/back.db -a illogical-control
  hub platform copy --from /tmp/back.db --to /data/control.db --replace | tee back-hub.txt
  diff <(copied back-platform.txt) <(copied back-hub.txt) && echo same
  fly secrets unset -a illogical-control ARUGULA_PLATFORM
  hub maintenance off --db /data/control.db
  shred -u back.db
  ```
  Always hold the platform before the hub runs in process again: two
  cloud workers on two files would both drive Managoat.

#### The 14 days after

The hub's copies of the platform's tables stay in `control.db`, read-only,
as the rollback: no hub code names a platform table
(`db::tables::the_hub_names_no_platform_table`), and running out of process
the hub never opens the platform's schema
(`the_hub_leaves_the_platforms_old_copies_alone`). To see that they really
haven't moved, now and then:

```
hub platform checksums --db /data/control.db > hub-later.txt
diff <(copied window-hub.txt) <(current hub-later.txt) && echo untouched
```

Keep the hub's cloud secrets (`ARUGULA_MANAGOAT_API_KEY` …) for the same 14
days: the in-process rollback needs them. After 14 days a follow-up drops
the copies and unsets those; the rollback then always copies
`platform.db` back first.

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

### Shared Docs

A person shares one Doc with a link (#1064). The machine encrypts the
snapshot (AES-256-GCM, the key only in the link's `#` fragment) and pushes
it in parts of at most 1 MB; control stores them and serves them to anyone
with the link. **Control holds ciphertext only and can't read it.**

- **Two tables** in `control.db`: `doc_links` (`id`, `account`, `daemon`,
  the served `version`, `parts`, `size`, `made`, `updated`, a rough
  `opens`) and `doc_link_parts` (BLOB rows keyed `(id, ver, n)`, `at`).
  A grep of the database finds no title or text of a shared Doc. Deleting
  a link, a machine (`drop_daemon`) or an account deletes its rows.
- **Limits**, per account: 200 MB of shared Docs in all and 100 links; per
  link, 48 MB of ciphertext (every Doc fits). A part is refused, with
  nothing stored, when it would pass the link's 48 MB or the account's
  200 MB, counting every half-uploaded version of every link the account
  has, and when the account already has 100 links. A commit checks them
  again.
- **The disk guard:** while the database's disk is over 80% full, control
  refuses new parts (507, "control's disk is over 80% full"). Parts that
  were never committed are swept after a day.
- **The volume** is 10 GB, since 200 MB an account is more than the
  1 GB it was made at ("The volume" above).
- **What it costs in R2** (Litestream, from R2's public prices: storage
  $0.015 a GB-month, $4.50 per million writes, no charge for reads out):
  ciphertext doesn't compress, and Litestream keeps a snapshot of the
  whole database a day for 7 days, so each GB of shared Docs held costs
  about 8 GB in the bucket, 12 cents a month. Each part is one or two
  writes, and a 43 MB Doc about 50 to 100: a fraction of a cent. Fly's
  own volume snapshots (5 days) hold them too, and add to the volume's
  cost, not R2's.
- **Backups and Unshare:** an unshared Doc's ciphertext stays in the
  backups until they age out, a week at most (Fly's snapshots, 5 days).
  Anyone who kept the link could decrypt it from a restored backup, so
  **Update isn't a retraction**.
- **What control learns:** that a Doc of this size, from this account and
  machine, was shared, updated, opened (roughly how often: previews and
  bots count) and unshared, and when. Not its title or content. The
  privacy notice says so (it lives on the site, not in this repo).
- **Per address,** the docs host's data reads are limited to 12,000 an
  hour (a 48-part Doc is about 50); a repeat visit of a version is a 304.
- **`?v=<ver>` on a part** (`…/data/<n>?v=<ver>`, #1066): control answers
  404 unless that is the version it serves, so a page that began on one
  version, when an Update lands mid-load, starts over from `meta` and never
  mixes two. The viewer page (`web/doc.html`) loads no font, so the CSP
  needs no `font-src`.

### Backups (Litestream to Cloudflare R2)

The hub backs up `control.db`, and the platform (M141) its own
`platform.db`, the same way (`arugula_control_common::backup`): each with
its own `LITESTREAM_*` secrets, under its own path in the bucket
(`LITESTREAM_PATH`, `control` and `platform` unless set; the platform
refuses `control`). The steps below are the hub's; the platform's are the
same with `-a arugula-platform` ("The platform's secrets", in the window's
runbook), and its restore recipe has `path: platform` and
`/data/platform.db`.

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
  -s 10`), point the machine at it (or destroy the old volume), and start
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

### Checking a migration on a copy

Control migrates its database every time it opens it. To see what a new
version does to production's data before deploying it, restore a copy as
above to `/tmp/control.db`, then open it with the new build and stop it
once it's listening (on another port, with nothing else set):

```
arugula-control --db /tmp/control.db --listen 127.0.0.1:7699
```

**The platform's tables (M142)** change only by numbered migrations
(`crates/platform/migrations/`). Opening logs `the platform's schema` with
the version it adopted (a database made before migrations, checked table by
table, after the old schema code has run if it had to: `upgraded=true`) and
the ones it ran, and `SELECT * FROM platform_migrations` shows them. A
schema that still matches no migration, a migration edited since it ran, or
a database with a migration the build doesn't know that isn't additive stops
the start with the reason, and leaves the file as it was. Rolling back past
additive migrations is fine; past one that isn't, restore the database too.

**Projects (M129, #1125).** Every team has one default project, and every
Cloud workspace, team channel and cloud secret has a project; DMs, pane
threads and archives have none. Teams are the hub's; projects, Cloud and
chat are the platform's, so as with the grants check below, attach the
platform's file as `p` (before M141, `control.db` itself). Each of these
prints nothing when it holds:

```
sqlite3 /tmp/control.db <<'EOF'
ATTACH '/tmp/platform.db' AS p;   -- before M141: ATTACH '/tmp/control.db' AS p;
SELECT 'team without one default', t.id FROM main.teams t
  WHERE (SELECT count(*) FROM p.projects pr WHERE pr.team_id = t.id AND pr.is_default = 1) != 1;
SELECT 'workspace without a project', id FROM p.cloud_workspaces WHERE project_id IS NULL;
SELECT 'team channel without a project', id, scope FROM p.chat_rooms
  WHERE kind = 'channel' AND scope LIKE 'team:%' AND project_id IS NULL;
SELECT 'secret without a project', account, name FROM p.cloud_secrets WHERE project_id IS NULL;
SELECT 'room in a project that should not be', id, kind FROM p.chat_rooms
  WHERE project_id IS NOT NULL AND NOT (kind = 'channel' AND scope LIKE 'team:%');
EOF
```

A team channel whose team is gone would show up here as one without a
project: control drops a team's rooms with the team, so there shouldn't be
any.

**`cloud.drive` grants (M130, #1126).** Every approved machine holds a live
grant over its account unless its owner revoked it. Control doesn't record
which machine made a call, so "every machine that drove a workspace in the
last 30 days" is checked as every machine of an account that made a Cloud
workspace or opened a tab in that time. Machines are the hub's (`devices`,
`daemons` in `control.db`); grants and Cloud are the platform's (`grants`,
`cloud_workspaces`, `cloud_tabs`), in `control.db` too while the platform
runs in process, and in the platform's own `platform.db` once M141 moves
them. So the query attaches the platform's file as `p`: on a copy of each
(`fly ssh sftp get /data/control.db -a illogical-control`, and
`fly ssh sftp get /data/platform.db -a arugula-platform` after M141; before
it, attach `control.db` itself). It prints nothing when it holds. A
machine gets its grant within a feed tick of joining, or when it first
drives, so one that joined in the last second may show up:

```
sqlite3 /tmp/control.db <<'EOF'
ATTACH '/tmp/platform.db' AS p;   -- before M141: ATTACH '/tmp/control.db' AS p;
SELECT 'machine without a live cloud.drive grant', d.account, d.id, n.name,
       (SELECT max(g.revoked_at) FROM p.grants g WHERE g.subject_id = d.id) AS revoked_at
FROM main.devices d LEFT JOIN main.daemons n ON n.id = d.id
WHERE d.kind = 'daemon' AND d.approved = 1
  AND d.account IN (
    SELECT account FROM p.cloud_workspaces WHERE created > (strftime('%s', 'now') - 30 * 86400) * 1000
    UNION SELECT account FROM p.cloud_tabs WHERE created > (strftime('%s', 'now') - 30 * 86400) * 1000)
  AND NOT EXISTS (SELECT 1 FROM p.grants g WHERE g.subject_kind = 'daemon' AND g.subject_id = d.id
    AND g.power = 'cloud.drive' AND g.scope_kind = 'account' AND g.scope_id = d.account
    AND g.revoked_at IS NULL);
EOF
```

A row with `revoked_at` set is a grant its owner revoked, not a miss. Drop
the `d.account IN (…)` clause to check every machine.

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
  `cloud agent created/updated/deleted/imported`, `cloud run stopped`
  (outcome; a run left over from before runs were retired),
  `cloud request answered`, `cloud secret saved/deleted`,
  `cloud workspace created` (counts of repositories, secrets, packages and
  env vars), `cloud workspace settings saved` (how many changes a rebuild
  would make), `cloud workspace rebuilt` and `cloud workspace retried`
  (whether it interrupted turns, and whether it waited for a machine),
  `cloud workspace deleted`.
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
