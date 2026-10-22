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
  - **Permissions:** read for metadata, contents, checks, commit
    statuses and actions; read and write for issues and pull requests
    (M167: write is only for sending a person's draft, below); and the
    organization permission **Members: read** (M165: GitHub sends the
    member, membership, organization and team events only with it).
  - **Events:** pull request, pull request review, pull request review
    comment, issues, issue comment, check run, check suite, status,
    workflow run; and (M165) member, membership, organization,
    repository, team and team add. These six change who may read a
    repository: the platform drops the access answers it keeps (10
    minutes) when one comes, so a removed collaborator is cut off at
    once rather than within 10 minutes. Without them, that's how long it
    takes. Any event that carries a repository also marks a linked repo
    private when GitHub says it is.
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
  - **Which process reads which** (M161): signing in is the hub's, and the
    App itself (its webhook, key, access checks and tokens) is the
    platform's. With the platform in process (the default) one config
    has them all. With it out of process, the platform takes
    `GITHUB_APP_ID`, `GITHUB_APP_SLUG`, `GITHUB_APP_WEBHOOK_SECRET` and
    the private key, and the hub `GITHUB_APP_ID`, `GITHUB_APP_SLUG` (it
    tells daemons there's an App), `GITHUB_APP_CLIENT_ID` and
    `GITHUB_APP_CLIENT_SECRET` ("The GitHub App's secrets" below).
  - **What control keeps of a webhook:** the platform checks the signature
    (HMAC-SHA256, constant time) and records the delivery id for 7 days
    (`github_webhook_deliveries`), so a redelivery is known after a
    restart. Only `{provider, host, repo, number?, event, delivery}` goes
    to daemons: the hub relays it over their relay sockets. The logs have
    the event, delivery, repository and how many daemons heard; never the
    payload.
  - **Who hears what:** a daemon's account must have signed in with GitHub,
    and the App's installation for the repository must be that GitHub
    user's, or GitHub must list them as a collaborator on it (asked with
    an installation token; answers kept ten minutes). Both go by the
    numeric GitHub user id, not the login. Organization membership alone
    doesn't count, and passkey-only accounts hear nothing: their blocks
    poll. A machine watches at most 200 repositories and an account 400.
  - **Hosted boxes** ask `POST /api/daemon/github/token {repo}` (signed as
    the daemon) for an installation token scoped to that repository with
    read-only permissions; control (the platform, which the hub asks)
    keeps one until five minutes before it expires.
  - **Tokens** (M167, aru-085): one helper in the platform
    (`crates/platform/src/forge/app.rs`) mints every installation token,
    and nothing else asks GitHub for one. Each names one repository and
    the least it needs: read-only for reads and the mirror's ingest (kept
    until five minutes before they expire), and `issues: write` or
    `pull_requests: write` for one send (minted for that send, never
    kept, and revoked at GitHub, `DELETE /installation/token`, right after
    its one call). A token whose answer from GitHub doesn't cover exactly
    the repository asked for is revoked and refused. Every mint is a row
    in `github_token_mints`: installation, repository id, the permissions
    asked for, the account (or `platform`), what for, and when, never the
    token. A read token used again from the cache inside the platform adds
    no row; one handed to a hosted box again from the cache adds a row for
    that account, with the purpose `read (cached)`. Rows are kept a year,
    pruned hourly.
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
- **The platform page's host** (M151, optional): `--platform-host`
  (`ARUGULA_PLATFORM_HOST`, `app.arugula.io` for the hosted one) is where
  people use projects, chat and Cloud workspaces, on an origin apart from
  the shell's, which holds the device keys ("The platform page is on
  app.arugula.io, not on the keys' origin"). Off when unset, and then
  nothing changes on any host, except that control's page offers no chat
  page or Cloud workspaces: since M155 the shell (the daemon's embedded
  web client) draws none of their views, only links to the platform page.
  - A bare host name. Control refuses to start with one of its own sites,
    or a name under the block domain.
  - Its DNS points at **the hub**, never at the platform, which still has
    no public address: the hub routes the host to it by `Host`
    (`crates/control/src/platform_host.rs`). On Fly that's a name and a
    certificate on the hub's app (`fly certs add app.arugula.io -a
    illogical-control`); the DNS record itself is M158.
  - On that host control answers only the platform's routes (those
    `routes.toml` gives `process = "platform"`, exactly as on control's
    own site) and the platform page: any other GET or HEAD is the page's
    `index.html` or one of its files (`web/dist-platform`, embedded in the
    platform crate). Everything of the hub's (`/api/…`, `/auth/…`,
    `/github/…`, `/.well-known/…`, `/d/…`, `/control.json`), the
    platform's `/platform.json` and `/internal/…`, and every WebSocket is
    a 404.
  - With the platform in process the hub serves the page itself; out of
    process it forwards the page's GETs to the platform, signed like every
    forwarded call, and the platform serves its own embedded copy. Its
    page ships with the platform, not with a daemon release.
  - Every answer there has the platform page's own CSP (its own scripts
    and styles, `connect-src 'self'`, no `unsafe-eval`, no frames either
    way), `X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, HSTS
    and `Cross-Origin-Opener-Policy: same-origin`, and no `Set-Cookie`
    but that host's own session's. The hub drops any cookie a request
    brings there but that one: its own session is control's site's alone.
  - Signing in there (M152, "The hub signs people in to the platform
    page", `crates/control/src/app_signin.rs`): the page sends a visitor
    to `/signin/platform` on control's site, which redirects back with a
    one-time code (a minute, single use, bound to the return address and a
    PKCE verifier), and the page swaps it at `POST /api/app/session` on its
    own host for a host-only session (`__Host-arugula_app`). The hub keeps
    those sessions (`app_sessions`): each ends with the hub session it was
    made from, at once. `POST /api/app/signout` signs that page out alone.
  - **Putting it live** (M158, hosted control; each step is Jake's go):
    1. Release, so the platform's binary (which embeds the page) and the
       hub both carry it; the tag deploys the platform first, then the hub.
    2. DNS: a DNS-only CNAME `app.arugula.io` → `illogical-control.fly.dev`
       on Cloudflare (the zone's token), then `fly certs add app.arugula.io
       -a illogical-control` and wait for `fly certs show` to say Ready.
    3. `fly secrets set ARUGULA_PLATFORM_HOST=app.arugula.io -a
       illogical-control` (a hub restart). `/control.json` then says
       `"platform": "https://app.arugula.io"`, and the shell links out for
       chat and Cloud workspaces, old `#chat`/`#cloud` links redirect, and
       pushes open there.
    4. Check: `curl -sI https://app.arugula.io/` has the page's CSP and no
       `Set-Cookie`; signed in on control's page, `app.arugula.io` opens
       with one silent redirect; `/api/me` there is a 404.
    Rollback: `fly secrets unset ARUGULA_PLATFORM_HOST -a
    illogical-control`: the shell draws chat and Cloud itself again only
    on a build from before M155, so roll back the release too if M155 is
    in it.
  - `/control.json` says where it is (`platform`: the page's origin, with
    the public URL's scheme and port; `null` without one).
  - `--platform-static-dir` (control's, platform in process) and
    `--static-dir` (`arugula-platform`'s) serve a local build instead, as
    `just dev-control` does on `app.localhost`.
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
  `GITHUB_APP_CLIENT_SECRET`: the GitHub App, as far as the hub needs it
  (`main.rs`). Its client id and secret sign people in, since
  `GITHUB_CLIENT_ID` and `GITHUB_CLIENT_SECRET` aren't set; its id and
  slug say there's an App.
- `GITHUB_APP_WEBHOOK_SECRET`, `GITHUB_APP_PRIVATE_KEY`: the App's webhook
  and key, read here only while the platform runs in process. They're
  `arugula-platform`'s from M161 ("The GitHub App's secrets" below); the
  hub's copies stay only as long as a rollback to in process, or to a
  release before M161, may need them, and the hub logs at start that
  they're unused.
- `LITESTREAM_BUCKET`, `LITESTREAM_ENDPOINT` (`backup.rs`), and
  `LITESTREAM_ACCESS_KEY_ID`, `LITESTREAM_SECRET_ACCESS_KEY` (checked by
  `backup.rs`, read by Litestream from the environment it inherits).
- `ARUGULA_MANAGOAT_API_KEY`: Cloud workspaces' provider. Without it,
  Cloud workspaces are off.
- `ARUGULA_E2B_API_KEY`, `ARUGULA_E2B_URL` (`https://api.e2b.dev` unless
  set; loam's is `https://api.e2b.<loam domain>`): hosted workspaces'
  boxes (`compute/`, M176), the platform's only. Never sent to a client or
  put in a box. Not set yet: nothing drives a box before M179.
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
the relay, push, block and doc origins, the jump host, telemetry) and
the platform (`arugula-platform`:
chat, Cloud, billing, projects and grants, and the GitHub App's webhook
and key, M161). The platform runs in one of two ways, from the same binaries:

- **In process** (`ARUGULA_PLATFORM=in-process`, the default, and the
  rollback): inside the hub, as one Fly app. Production runs this way
  until M141's window.
- **Out of process** (`ARUGULA_PLATFORM=http://arugula-platform.internal:7700`, and through Flycast, `http://arugula-platform.flycast:7700`, from M145):
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
| Public | `control.arugula.io` (`[http_service]`, 7690) | nothing: one service, for Flycast, on a private IPv6 only (M144) |
| Private | its internal API, `[::]:7691` (`HubPort` for the platform) | `[::]:7700` (the forwarded routes, and `PlatformPort` for the hub) |
| Volume | `control_data` (10 GB) | `platform_data` |
| Health | `/control.json` through the proxy | `/platform.json`, the Flycast service's check |
| Logs | `fly logs -a illogical-control` | `fly logs -a arugula-platform` |

**How they reach each other.** Over Fly's private network (6PN, carried
over WireGuard between Fly's hosts). The platform calls the hub at
`http://illogical-control.internal:7691`. The hub calls the platform at
`http://arugula-platform.internal:7700` until M145's window, and from then
through Flycast, `http://arugula-platform.flycast:7700`: Fly's proxy on the
private network, which spreads calls across the platform's two machines
and sends them only to machines passing `/platform.json` ("Two platform
machines" below).
- The hub's internal API listens on its own port, which no `fly.toml`
  service names, so Fly's public proxy never reaches it. It also refuses
  anything carrying `Fly-Forwarded-Port` or `Fly-Client-IP` (the headers
  that proxy adds), in case someone ever declares a service for it, and
  every call has to be signed with the shared key.
- The platform's one service (`[[services]]` on 7700, plain TCP, no
  handlers) is for Flycast, which needs a **private** IPv6:
  `fly ips allocate-v6 --private -a arugula-platform`. Never
  `fly ips allocate-v4` or a public `allocate-v6` for it: the service
  would be on the internet. Deploys pass `--no-public-ips`, and
  `scripts/ci-platform-deploy` won't deploy while the app has a public
  address. Fly's proxy passes the TCP through untouched, and the platform
  trusts no address: only the hub's signed principal (and, for
  `PlatformPort`, the signed internal call).

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
health check passing. Then it builds the nightly dump's image around the
same binary and moves the dump machine onto it ("The nightly dump"). It needs the repository secret
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

**The GitHub App's secrets** (M161, #1207). The App's webhook still comes
to `control.arugula.io/github/webhook` (nothing changes on GitHub): the
hub's gateway forwards it to the platform, which checks the signature,
records the delivery (`github_webhook_deliveries`, 7 days, pruned hourly)
and writes a linked repo's rows (M164). The hub's
`/api/daemon/github/token` asks the platform (`PlatformPort::github_token`);
the token crosses the private network in the answer, signed like every
internal call, and is never logged. Until M174 the platform also handed
each webhook's pokes to the hub, which relayed them to the daemons
watching the repository; see "The poke relay, retired" below.

| Secret | `illogical-control` (hub) | `arugula-platform` |
|---|---|---|
| `GITHUB_APP_ID`, `GITHUB_APP_SLUG` | yes: there's an App, and its slug for daemons | yes |
| `GITHUB_APP_CLIENT_ID`, `GITHUB_APP_CLIENT_SECRET` | yes: sign-in with GitHub | no |
| `GITHUB_APP_WEBHOOK_SECRET`, `GITHUB_APP_PRIVATE_KEY` | only for a rollback (below) | yes |

The runbook, once, for the first release with M161. Its tag deploys the
platform first, then the hub; from the hub's restart on, the webhook goes
to the platform. So the platform must have the App's secrets **before**
that tag. Nothing here is printed.

1. **Stage the App's four secrets on `arugula-platform`**, copied from
   the hub's running process (as "The platform's secrets" in M141's runbook below: `secrets-check` names
   its pid N). `--stage` restarts nothing; the tag's deploy takes them.

   ```
   hub()  { fly ssh console -q -a illogical-control -C "/arugula-control $*" | tr -d '\r'; }
   hub platform secrets-check --db /data/control.db        # note N
   fly ssh sftp get /proc/N/environ hub.environ -a illogical-control
   python3 -I - hub.environ <<'PY' | fly secrets import --stage -a arugula-platform
   import sys
   want = ["GITHUB_APP_ID", "GITHUB_APP_SLUG", "GITHUB_APP_WEBHOOK_SECRET", "GITHUB_APP_PRIVATE_KEY"]
   raw = open(sys.argv[1], "rb").read()
   if not raw:
       sys.exit("hub.environ is empty: sftp read /proc as 0 bytes. Don't guess values; stop and ask.")
   env = dict(kv.split("=", 1) for kv in raw.decode().split("\0") if "=" in kv)
   missing = [k for k in want if not env.get(k)]
   if missing:
       sys.exit("not in the hub's environment: " + ", ".join(missing))
   for k in want:
       # The PEM's newlines as \n, which the platform reads back (parse_pem).
       print(f"{k}={env[k].replace(chr(10), chr(92) + 'n')}")
   PY
   shred -u hub.environ
   fly secrets list -a arugula-platform     # the four names, digests only
   ```

   Infisical (`arugula` prod) has the same values if the hub's process
   can't be read.
2. **Leave the hub's secrets as they are.** `illogical-control` keeps
   `GITHUB_APP_ID`, `GITHUB_APP_SLUG`, `GITHUB_APP_CLIENT_ID` and
   `GITHUB_APP_CLIENT_SECRET` for good (sign-in; the id and slug say
   there's an App), and `GITHUB_APP_WEBHOOK_SECRET` and
   `GITHUB_APP_PRIVATE_KEY` for now: a rollback to in process, or to a hub
   from before M161, answers the webhook itself and needs them.
3. **Release.** The tag deploys `arugula-platform`, then
   `illogical-control` (release.yml's `platform`, then `control`).
4. **Check.**
   - `fly logs -a arugula-platform` shows `GitHub App` at start, and no
     `no GITHUB_APP_ID` warning.
   - In the App's settings (Advanced, Recent Deliveries), redeliver a
     recent delivery: `202`, and the platform's log has `webhook handed to
     the hub` with its delivery id. Redeliver it again: `200`,
     `{"duplicate":true}`. (Since M174 the answer is `{"taken":true}` and
     there's no `handed to the hub` line.)
   - A machine with a PR block open still hears pokes (its block says
     live). Not since M174.
5. **Later**, once no rollback past M161 or to in process is planned,
   take the hub's copies away (a restart):
   `fly secrets unset -a illogical-control GITHUB_APP_WEBHOOK_SECRET GITHUB_APP_PRIVATE_KEY`.

If the platform starts without the App, it warns at start (`no
GITHUB_APP_ID`), and every webhook is refused with 503 and `this control
has no GitHub App: the platform has no GitHub App key`, logged as an error
on the platform and as `the platform answered with an error` on the hub.
GitHub keeps the failed deliveries, to redeliver once the secrets are set
(then restart the platform). The hub also logs an error when a hosted
box's token finds the platform without an App while it has
`GITHUB_APP_ID`. Daemons' blocks poll meanwhile.

**The poke relay, retired** (M174, #1220). From M40 to M174 a daemon told
the hub which github.com repos it had blocks on (`forge.watch` on its relay
socket), heard every minute where each stood (`forge.watching`, the
heartbeat), and heard the App's webhooks as `forge.poke`, so its blocks
polled at once and slowly between. Since M172 a linked repo's blocks are
pins onto the platform's mirror, and since M174 the relay is gone:
- The hub answers every `forge.watch` with an empty
  `{"t":"forge.watching","repos":[]}`, and says nothing else: no
  heartbeat, no pokes, no `forge-watch` rate limit. A daemon told nothing
  is live on nothing, so its blocks poll GitHub with its own `gh` login,
  as they do on a tailnet without control. No daemon errors on it.
- The platform's webhook writes rows and answers `202 {"taken":true}`. It
  hands nothing to the hub.
- `HubPort::forge_pokes` stays, answering 0, while a platform from before
  M174 might call it (one rolled back past the hub): its webhooks still
  answer GitHub 202, not 502. `PlatformPort::forge_standing` stays while
  a hub from before might call it (the platform deploys first). Both go a
  release later.
- `/api/daemon/github/token` (hosted boxes' read token) is not part of the
  relay, and stays.
- Forgejo and GitLab blocks' own webhooks to the daemon are unchanged.

Deploy order is the usual one, platform first. A new platform with an old
hub: the hub keeps answering watches as before, but no pokes reach it.
An old platform (a rollback) with a new hub: its pokes are taken, and go
nowhere.

**The App's write permissions** (M167, #1213). The App gains
`issues: write` and `pull_requests: write`, and nothing else, so a draft
can be sent with every machine asleep (M168). The code asks for write
only inside a send, so the upgrade can go out before or after the release
that has it. Do the test App first, then the real one:

1. **The test App** (`arugula-forge-tests`, App id 5280040, on the
   `arugula-salad-test` organization): at
   `https://github.com/organizations/arugula-salad-test/settings/apps/arugula-forge-tests/permissions`,
   under *Repository permissions*, set **Issues** and **Pull requests**
   to *Read and write*. Leave everything else as it is. Save, and confirm
   the prompt that installers will be asked.
2. Approve it on the test installation: the organization's *Settings →
   GitHub Apps → arugula-forge-tests → Review request* (an org owner).
   Until then its tokens keep the old permissions, and GitHub refuses a
   mint that asks for write (`422`), so a send fails clearly and its
   draft is kept.
3. Run `forges-nightly` by hand on `main` and check it passes.
4. **The real App** (`illogical-by-arugula`): the same two permissions,
   in its settings on the account that owns it. GitHub emails every
   installer to approve. Jake approves his own installations; a team's
   org owner approves theirs, after the note to installers.
5. Check: on an installation's page (*Settings → Applications →
   Installed GitHub Apps → Configure*), Issues and Pull requests say
   *Read and write*, and nothing else changed. Until an installer
   approves, sends to their repositories fall back to a machine (M168),
   and reads carry on as before.

**The App's access events** (M165, #1211). A separate change, with its
own approval: the App subscribes to the events that change who may read
a repository, and gains the organization permission **Members: read**,
which the member, membership, organization and team events need. Until an
installer approves, a removed collaborator keeps reading a private
repo's items for up to 10 minutes after their access goes (the kept
answer's age), rather than until the next event; nothing else changes.
Do the test App first, then the real one:

1. **The test App** (`arugula-forge-tests`): on its permissions page
   (the link above), under *Organization permissions*, set **Members**
   to *Read-only*. Under *Subscribe to events*, tick **Member**,
   **Membership**, **Organization**, **Repository**, **Team** and
   **Team add** (where GitHub lists it). Leave everything else as it
   is, save, and confirm the prompt that installers will be asked.
2. Approve it on the test installation (the organization's *Review
   request*, an org owner).
3. Check: remove a collaborator from `forge-tests` on GitHub and look
   for a `member` delivery in the App's *Advanced → Recent Deliveries*,
   answered 202.
4. **The real App** (`illogical-by-arugula`): the same permission and
   events. GitHub emails every installer to approve; a user account's
   installation has no organization members, and approving changes
   nothing else for it.

**Sending as a person** (M168, #1214; `docs/control.md`). A send's user
token comes from the App's own OAuth client, which the hub holds
(`GITHUB_APP_CLIENT_ID`/`SECRET`, as for sign-in), through the App's one
callback, `/auth/github/callback`: nothing to change in the App's
settings. A user token can do what both the App and the person may, so
until an installation approves `issues: write` and `pull_requests: write`
(above), GitHub refuses its call and the send goes through one of the
person's machines instead. To follow a send:

```sh
fly logs -a illogical-control | grep -E "a send's GitHub authorization|a send came back|revoking a send"
fly logs -a arugula-platform | grep -E "a draft sent as its person|a send failed|no way to send|user token refused"
```

The logs name the account, the draft and how it went (`via` token or
machine); never a token or a code. Each user token is a
`github_token_mints` row with `kind = 'user'` and the purpose
`send:comment`, `send:review` or `send:issue`.

**The forge mirror** (M164, #1210). A linked repo's PRs and issues are
platform rows (`forge_items`, `forge_checks`, `forge_events`): metadata,
never a body or a comment's text.
- **Webhooks write rows.** `pull_request`, `pull_request_review`,
  `pull_request_review_comment`, `issues`, `issue_comment`, `check_run`
  and `status` events for a linked repo write its rows in the webhook's
  own request. A row is only overwritten by a
  payload whose GitHub `updated_at` is newer, so a late redelivery
  changes nothing. GitHub's times are whole seconds: two payloads from
  the same second that disagree leave the row as it was and put the item
  (or the head's checks) in `forge_refetch`, which the poller reads again
  first thing, at once. A commit status is kept for any commit (it may
  come before its PR's new head); the poller prunes checks on a commit
  no item has as its head after an hour. An issue deleted or transferred
  goes, with its events and inbox rows; its drafts stay (M170's). An
  unlinked repo's events write nothing.
- **One poller per repo**, leased like Cloud's jobs (`linked_repos.lease`,
  `FOR UPDATE SKIP LOCKED`: two machines never poll one repo). Its first
  turn after linking backfills: open PRs and issues, everything updated
  in the last 90 days, open PRs' checks, and reviews as events. Then it
  reconciles every 5 minutes (`ARUGULA_FORGE_RECONCILE_MINUTES`): what
  GitHub lists as updated since the last one, with two minutes' overlap.
  Every turn also reads the repo itself, keeping `linked_repos.full_name`
  and `private` current. A turn reads at most 50 list pages; one with
  more keeps a cursor (`linked_repos.poll_cursor`) and the next turn
  carries on from it. A row it finds missing or wrong is **drift**: a
  webhook that never came or failed. It fixes the row, and counts it when
  GitHub's change came after the last reconcile and more than a minute
  ago (an old PR's earlier reviews are history, not drift). It logs
  `forge mirror: drift fixed` with the repo id, the item or check, and
  the fields (never a title). Its tokens are read-only, for the one repo,
  logged with purpose `ingest`.
- **Failures and uninstalls.** A failed turn waits a minute, doubled for
  each failure in a row (`linked_repos.poll_failures`), up to an hour.
  When GitHub won't mint a token for the repo (404, 422), or the repo's
  own read says 404 or 451, the poller asks GitHub about the installation
  as the App: deleted or suspended unlinks all its repos, otherwise the
  repo is taken out of it. Either way the purge follows, as if the
  installation's webhook had come (`forge mirror: gone from the App,
  unlinked`). The leader also sweeps `GET /app/installations` every 6
  hours and unlinks installations GitHub no longer lists, or lists as
  suspended (`forge mirror: swept the App's installations`).
- **The rate budget.** Each installation's `x-ratelimit-*`, as last
  heard. The poller leaves 30% of the limit (`ARUGULA_FORGE_RATE_RESERVE`,
  in percent) for reads on demand and sends: below that it stops its
  turn, gives the lease up, and the repo waits a minute, until the limit
  resets.
- **Bodies and comments** (M166, #1212) aren't rows unless a project
  turns on content mirroring for the repo. Opening an item reads them from
  GitHub (tokens with purpose `read`, for the reader), kept about 3
  minutes in each machine's memory; webhooks for the item drop them on
  every machine (`forge_items.content_rev`). Each account fetches at most
  300 an hour, and only an installation with none of its limit left is
  refused. With mirroring on, webhooks and the poller write
  `forge_content` (the next turn after turning it on is a backfill), and
  turning it off deletes the rows in the same transaction. No log line
  holds a body or a comment: content paths log repo ids, numbers and
  counts, and a failed write's error is cut before Postgres's `DETAIL`
  (which would show the row). The nightly dump logs only table names, row
  counts and checksums.

**Reading the mirror's numbers.** Each platform machine logs them every
5 minutes, from memory (a restart starts them again):

```sh
fly logs -a arugula-platform | grep 'forge mirror:'
```

- `forge mirror: webhook to row`: over the newest 1000 deliveries that
  wrote a row, `arrival_p50_ms`/`arrival_p95_ms` (from the request
  reaching the platform to the row's commit) and
  `event_p50_ms`/`event_p95_ms` (from GitHub's own time for the change,
  in whole seconds, so up to a second pessimistic). The gate is
  `event_p95_ms` under 5000.
- `forge mirror: drift today`: one line per linked repo, its `items`,
  `drift` fixed today (UTC, this machine's), and `per_thousand`. The gate
  is under 1 a day; add the machines' counts up, or count the day's
  `drift fixed` lines, which every machine logs.
- `forge mirror: rate budget`: one line per installation, `limit`,
  `remaining`, `used_pct`, `reserve_pct`, and `backing_off` when the
  poller is leaving the reserve be. `forge mirror: poller left the rate
  reserve be` is logged each time it does.
- `forge mirror: poller turn`: a backfill, or a reconcile that fixed
  rows, with `fixed` and `drift`.

**The inbox** (M171, #1217). Each member's inbox (`inbox_items`) is
worked out from the mirror's rows by the block's attention rules: a
review requested, checks failed, changes requested, mentioned (only where
the project mirrors content), assigned, done, and a draft waiting. A
webhook's rows mark their items, each poller turn marks its whole repo
(a backfill's quietly), and each machine works out what it marked within
half a minute. A reason newly holding pushes once, through the hub, only
to members the viewer check lets read the item, and only for what changed
in the last hour. `inbox push` lines name the item and the account, never
a title.

**The inbox's shadow period.** For two weeks the server's inbox is held
up against forge blocks' attention on Jake's machines. Set
`ARUGULA_INBOX_SHADOW=1` in each of those daemons' environment (`on`,
`true`, or a number of minutes; 15 minutes for `1`) and restart them. It's
off unless set. Each then sends `POST /api/inbox/shadow` with its open
GitHub blocks on linked repos: the item, the reasons the block's rules
find (whether or not the person looked), and the platform's drafts it
shows. No titles or text. The platform compares each with the owner's
inbox rows for that item (seen, done or not) and logs:

```sh
fly logs -a arugula-platform | grep 'inbox shadow'
```

- `inbox shadow`: one line per report, with `day` (UTC), `daemon`,
  `compared`, `agree` and `disagree`. A day's numbers are the sum of its
  lines on every machine; the match rate is `agree / compared`. The gate
  is 99%.
- `inbox shadow: disagree`: one line per disagreement, with the `item` id
  and both sides' reasons (`block`, `platform`). Expect some, and explain
  them: a team's review request (the platform doesn't know a person's
  teams), mentions where content isn't mirrored, a block's poll or the
  platform's recompute catching up, and `mergeable` (the platform has only
  GitHub's `mergeable_state`).

Fly keeps logs only a few days, so ship them where they last (or collect
the lines daily) for the two weeks.

**M173 go-live** (#1219). Turning the mirror on for real people's repos is
Jake's go. Until then nothing is linked, so the platform mirrors nothing.
In order:

1. **Before.** The release with M159–M172 is on both apps. The real App's
   write permissions and access events are approved on Jake's
   installations ("The App's write permissions", "The App's access events"
   above). The privacy notice and the site's forge page are updated the
   same day (arugula-salad/site): they reverse the forge page's "only
   'something changed'".
2. **Link 10 or more repos,** Jake's and one team's. Jake, signed in with
   GitHub, on each project's settings page on the platform page, or:

   ```sh
   arugula project link jhgaylor/<repo>                          # Jake's own project
   arugula project link <org>/<repo> --project <team's project>  # a team owner or editor
   arugula project ls                                            # --project for the team's
   ```

   The team's org owner installs the App on their organization first (a
   link answers `install_app`, with the page, until they do). Leave content
   mirroring off unless the team asks for it. Each repo's first poller turn
   is a backfill: `forge mirror: poller turn` with `backfill` and its
   repo id, within a few minutes.
3. **The inbox shadow on Jake's machines.** In each daemon's environment,
   `ARUGULA_INBOX_SHADOW=1`, then restart the daemon:
   - Linux: `systemctl --user edit arugulad` with
     `[Service]` and `Environment=ARUGULA_INBOX_SHADOW=1`, then
     `systemctl --user restart arugulad`.
   - macOS, `arugulad install`'s agent: the same key under
     `EnvironmentVariables` in `~/Library/LaunchAgents/arugulad.plist`
     (reinstalls keep it), then
     `launchctl kickstart -k gui/$(id -u)/arugulad`.

   Check each: an `inbox shadow` line naming its `daemon` within 15
   minutes. Its machine needs forge blocks open on linked repos to
   compare anything; a shadowing daemon doesn't pin, so its blocks keep
   reading GitHub.
4. **Read the numbers every day,** from the lines above
   ("Reading the mirror's numbers", "The inbox's shadow period"):

   | Number | Line | Field | Holds when |
   |---|---|---|---|
   | Webhook to row | `forge mirror: webhook to row` | `event_p95_ms` | under 5000 |
   | Drift | `forge mirror: drift today` (every machine) | `drift` over `items`, summed | under 1 per 1000 a day |
   | Rate budget | `forge mirror: rate budget` | `used_pct`, `backing_off` | never backing off for long |
   | Inbox shadow | `inbox shadow` (every machine) | `agree / compared`, summed | 99% or more |

   Fly keeps only a few days of logs, so keep each day's lines:

   ```sh
   fly logs -a arugula-platform --no-tail | grep -E "forge mirror:|inbox shadow" > m173-$(date -u +%F).log
   ```

   The lines carry repo and item ids, never titles; keep the files off
   the public repo anyway.
5. **Post the numbers on #1204** for two weeks: each day's four numbers,
   the repo count, and every `inbox shadow: disagree` explained (the kinds
   listed above). M173 is done when they hold for two weeks.
6. **Stop** if installers refuse the permission upgrade, or a team objects
   to content custody (#1204). `arugula project unlink` stops reading a
   repo at once and purges its rows within a day; pinned blocks go back to
   reading GitHub with their own login.

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

**Where a request's time goes.** Every platform route answers with a
`Server-Timing` header: the platform's entries
(`crates/platform/src/timing.rs`), then the gateway's
(`crates/control/src/gateway.rs`). Durations are in ms; `desc` is a
count. Only durations and counts: never a path, an id or anything the
request carried.

| entry | who adds it | what |
|---|---|---|
| `db;dur=` | platform | time in database calls, waiting for a connection included (a Postgres pool slot, SQLite's lock) |
| `dbq;desc=` | platform | statements sent; on Postgres each is a round trip, `BEGIN` and `COMMIT` too |
| `dbprep;desc=`, `dbconn;desc=` | platform | statements prepared and connections opened (one more round trip, or a handshake, each); only there when not 0, which is the steady state |
| `hub;dur=`, `hubn;desc=` | platform | `HubPort` calls and how many, the `x-arugula-hub-seq` catch-up (`cache::fresh`, at most 250 ms) included; out of process each is an ewr↔iad round trip |
| `app;dur=` | platform | the platform's whole time, until its answer begins |
| `up;dur=` | gateway | from sending the call until the platform's answer began: the private network, Flycast's proxy and `app` |
| `gw;dur=` | gateway | the gateway's whole time until the answer begins, `up` included (who's asking, the seq header, signing) |

So `up - app` is the private network plus Flycast, `gw - up` the hub's own
work, and whatever the client measured beyond `gw` is between it and the
hub (TLS, Fly's edge). In process only the platform's entries are there.

From any machine logged in with the CLI (`arugula login`),
`ARUGULA_DEBUG_HTTP=1` prints each call's total and its header on stderr:

```
ARUGULA_DEBUG_HTTP=1 arugula grants >/dev/null
# arugula: GET /api/grants: 200 in 84.2 ms; server-timing: db;dur=0.91, dbq;desc=1, hub;dur=0.00, hubn;desc=0, app;dur=1.20, up;dur=9.80, gw;dur=10.40
```

Both processes also log every request at `debug` with the same numbers
(the platform's `platform request`, with the route's pattern; the hub's
`forwarded`, with the request id). They're off at the default `info`:
`fly secrets set -a arugula-platform RUST_LOG=arugula_platform=debug`
(and `-a illogical-control RUST_LOG=arugula_control=debug`) turns them on,
at a restart each; unset to turn them off.

### Two platform machines (M144)

On Postgres the platform runs two machines; on SQLite exactly one
(`packaging/platform/fly.toml`, "Two modes"). `scripts/ci-platform-deploy`
refuses to deploy more than one machine without the
`ARUGULA_PLATFORM_DATABASE_URL` secret, since each would have a database
of its own. A deploy replaces one machine at a time (`[deploy] strategy =
"rolling"`), each waiting for `/platform.json`, which answers 503 while the
machine can't reach its database and says whether it leads
(`{"database": true, "leader": false, ...}`). A stopping machine takes no
new Cloud job and waits up to 10 s for the ones it holds to checkpoint, so
they go to the other machine at once, not when their leases run out.

**What the two share, and how.**

- **Cloud jobs** (runs, Cloud workspaces, tabs): every machine runs four
  workers. A claim is `SELECT … FOR UPDATE OF j SKIP LOCKED` on Postgres,
  so two machines never take the same job and neither waits on the
  other's; the lease (`cloud_jobs.lease`, 120 s) is written only if the job
  is still free. A machine that dies holding a job leaves it to the other
  when the lease runs out; every checkpoint checks the lease, so a worker
  that comes back late can't overwrite its successor.
- **Chat's live events**: each is a row of `chat_events`, numbered across
  both machines; `pg_notify` wakes the other machine, which sends its own
  streams what it hadn't (`chat::Hub::catch_up`), and every machine reads
  them in every 5 s anyway. The notification carries only the number
  (`chat <seq>`, far under `NOTIFY`'s 8000 bytes): the machine reads the
  rows. A page's `Last-Event-ID` is `c-<seq>`, the same on both, so a page
  that drops resumes on whichever machine it reaches, from the database,
  while the 10,000 newest are kept; otherwise it resyncs.
- **The listening connection**: each machine holds one connection of its
  own, outside its pool of four, for `LISTEN` and the leader's lock. It's
  made again whenever it's lost (backing off to 10 s), then reads what it
  missed. Both ends send TCP keepalives, so a machine that vanishes without
  closing it loses the lock within about a minute.
- **The leader** is the machine whose listening connection holds
  `pg_try_advisory_lock` (`leader`): it alone applies the hub's change feed
  to projects, team members and grants, and reports billing's usage. The
  other tries for it every 5 s. Everything the follower writes is
  idempotent and checks the hub's current answer, and `hub_feed` only moves
  forward, so a moment with two leaders (a cut-off leader that hasn't
  noticed yet) costs nothing.

Both need the cluster's direct address, not PgBouncer's in transaction
mode, which can't hold a `LISTEN` or a session's lock.

**Locks** (M143 took one advisory lock in every transaction, so the two
machines' writes took turns; M144 replaced it). What's left:

| Lock | Where | Why no row can do it |
|---|---|---|
| `migrations` (transaction) | `migrate::run_postgres` | two machines starting together run the migrations once |
| `chat_events` (transaction) | `Db::add_chat_event` | events commit in `seq` order, so a machine that has sent up to N never passes one still uncommitted below it |
| `leader` (session) | the listening connection | one machine applies the feed and reports usage |
| `window` (transaction) | `copy --to postgres://` | two copies at once wait for each other |

Row-level instead of the old lock:

| What | How |
|---|---|
| Cloud claims | `FOR UPDATE OF j SKIP LOCKED`, and the lease written only `WHERE lease_until <= now` |
| A worker's checkpoint, finish and events | `WHERE lease = ?` (events: the job's row `FOR UPDATE`), so a reclaim waits for the commit |
| Saving settings; taking or cancelling a waiting rebuild | the Cloud workspace's row `FOR UPDATE` |
| Opening a tab while its workspace is deleted | the workspace's row `FOR UPDATE`: the delete waits, or is seen |
| The same request key on both machines | the key's unique index; the loser reads the winner's |
| Relaunching a tab that's being closed | `status NOT IN ('closing','closed')`, rechecked on the locked row |
| Numbering a message | `UPDATE chat_rooms … RETURNING last_seq` (as before) |
| Editing or deleting a message | the message's row `FOR UPDATE` (the room's byte count) |
| Importing a machine's old threads | the room's row `FOR UPDATE` |
| Sending an approval push | each push taken for 30 s (`UPDATE … WHERE next_attempt <= now`) before it's sent |
| Giving `cloud.drive` back | `grants_live`'s unique index, `ON CONFLICT DO NOTHING`, then read |
| Default projects, grants on join | unique indexes, `ON CONFLICT DO NOTHING` (as before) |
| `hub_feed` | only moves forward |

Soft on two machines (each was exact while one writer ran at a time): the
100 custom agents per account and chat's per-team and per-machine room and
byte limits can be passed by as many requests as arrive at the same
moment; a workspace that names a secret deleted at that moment fails to
fill its vault and says so.

**What each machine keeps to itself:** its rate limits (`Limits`: with
requests landing on both, an account gets up to twice each limit), its
cache of the hub's answers (each machine follows the hub's feed, with the
same `cache::DELAY`), its live streams and their caps (20 per account,
5,000 in all, per machine), chat's one push per room per person a minute,
and whether the person has the room open (a page open on the other machine
doesn't stop a push), and its analytics queue (flushed on stop).

**How the hub reaches them: Flycast** (Jake, M144). The hub's
`ARUGULA_PLATFORM` is `http://arugula-platform.flycast:7700`, the app's
private IPv6 (`fly ips allocate-v6 --private -a arugula-platform`; never a
public one), where Fly's proxy passes each connection to a machine passing
its `/platform.json` check. During a rolling deploy the proxy stops
sending to the machine being replaced. With no healthy machine at all, the
proxy takes the connection and closes it unanswered: the gateway treats a
`GET` or `HEAD` closed that way like a platform it can't reach (tried for
5 s, then the maintenance 503), and anything else as a 502, since it may
have arrived. `[services.concurrency]` allows 5,000 connections a machine:
each open chat page holds one (its live stream), and Fly's default of 25
would refuse the rest. Nothing on either side depends on the caller's
address. The internal calls' nonces are remembered per machine, so a call
captured on the private network could be replayed once on the other
machine within its 60 s; the principal header was never single-use.

Before M145, with one machine on SQLite, the hub calls
`http://arugula-platform.internal:7700` directly (6PN, no proxy). The
switch is step 11 of M145's window ("The second window (M145)", below).

**Switching the hub to Flycast** (M145's window, after the platform runs
on Postgres; each step needs Jake's go):

```
fly ips allocate-v6 --private -a arugula-platform     # once; never a public one
fly ips list -a arugula-platform                      # only a private_v6
fly secrets set -a illogical-control ARUGULA_PLATFORM=http://arugula-platform.flycast:7700   # one hub restart
```

The hub's restart drops relay sockets for a moment, which reconnect on
their own, as at M141's flip; it's best done inside the window, with the
maintenance switch on. Check: platform routes answer (a chat list, a Cloud
list), `fly logs -a arugula-platform` shows calls on both machines, and
the hub logs no `the platform can't be reached`. **Roll back:**
`fly secrets set -a illogical-control ARUGULA_PLATFORM=http://arugula-platform.internal:7700`
(one hub restart). The private IPv6 can stay; it's reachable only on the
private network. Release it with `fly ips release <address> -a arugula-platform`
if Flycast is abandoned.

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
everything that's the hub's own (`GITHUB_APP_*`, which M161 later split:
"The GitHub App's secrets"; `SPRITES_TOKEN`).
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

### The second window (M145)

The platform's tables move from `platform.db` on its volume to Fly Managed
Postgres (`arugula-platform-db`, iad, PG17), and then the platform runs two
machines that the hub reaches through Flycast (#1162). Chat and Cloud answer
the maintenance 503 for the few minutes it takes, while terminals, the relay
and sign-in don't notice. This is rehearsed end to end with
`testnet/window2.sh` ([testnet/README.md](../testnet/README.md)), including
both rollbacks, the first nightly dump and the restore drill.

**The tools** are the first window's, run on the platform's machine, which
reaches the cluster over 6PN, so nothing passes through a laptop
(`crates/platform/src/window_portable.rs`):
- `copy --from F --to T [--replace]` works between a file and Postgres,
  either way, in one transaction. It's proven with a **portable checksum**
  that both databases give alike: SHA-256 over each row's columns in name
  order, each value null, integer, real, text or bytes, with booleans as
  0/1 and the row digests sorted. On Postgres the identities are moved on
  past the copied rows.
- `checksums --db F|URL` (`--portable` for a file) and
  `secrets-check --db F|URL` only read. On Postgres they never migrate: a
  database this build would migrate is refused.
- `migrations --db F|URL` prints what a database has applied against this
  build, without running anything.
- **A URL is never typed.** On the command line it is `@/tmp/pg.url`, a
  file put there with `fly ssh sftp put` (a restart clears `/tmp`). After
  the flip it is `env:ARUGULA_PLATFORM_DATABASE_URL`, the running server's
  own. Output prints a URL without its password.

**What has to be true first:**
- A release with this runbook's tools (M144's copy, plus `migrations`,
  `@FILE`/`env:` and `checksums --snapshot`) is deployed to the platform.
  The platform runs on SQLite, one machine, and Litestream replicates under
  `platform`.
- `ARUGULA_PLATFORM_DATABASE_URL` (the direct address, `sslmode=disable`)
  is in Infisical (`arugula` prod, `/illogical/control`) and isn't set on
  the app. Don't stage it ahead of time: a deploy between staging and the
  window would start the platform on an empty Postgres.
- The nightly dump's keys exist ("The nightly dump", below: the age
  keypair, and the platform's `LITESTREAM_*` escrowed in Infisical for the
  drill). They're only needed after the window, for step 13.

Keep two terminals on the logs throughout: `fly logs -a illogical-control`
and `fly logs -a arugula-platform`. Paste these helpers first, where
arugula's `.infisical.json` is:

```
hub()  { fly ssh console -q -a illogical-control -C "/arugula-control $*" | tr -d '\r'; }
plat() { fly ssh console -q -a arugula-platform ${M:+--machine $M} -C "/arugula-platform $*" | tr -d '\r'; }
sums()   { awk 'NF == 3 && length($3) == 64 { print $1, $2, $3 }' "$1" | sort; }   # checksums' output
copied() { awk 'NF == 6 && $NF == "ok" { print $1, $3, $5 }' "$1" | sort; }         # a copy's "to" side
cut16()  { awk '{ print $1, $2, substr($3, 1, 16) }'; }
# The cluster's direct URL into the platform machine's /tmp, printing nothing.
put_url() {
  infisical secrets get ARUGULA_PLATFORM_DATABASE_URL --env prod --path /illogical/control --plain --silent > pg.url
  fly ssh sftp put pg.url /tmp/pg.url -a arugula-platform ${M:+--machine $M} >/dev/null
  shred -u pg.url
}
mkdir -p ~/m145 && cd ~/m145 && umask 077
M=$(fly machine list -a arugula-platform --json | jq -r '.[] | select(.config.metadata.fly_process_group == "app") | .id')
echo "$M"     # exactly one id: the machine with the volume, M1 from here on
```

#### The notice (a day ahead)

As in the first window (its "The notice" has the commands), with this text:

```
cat > notice.txt <<'EOF'
Chat and Cloud maintenance: <Day> <HH:MM> UTC
Chat and Cloud will be down for about 15 minutes from <HH:MM> UTC on <Day>, while they move to a new database. Terminals, the relay and signing in keep working, and agents' calls wait it out. Afterwards, reload any control page you have open.
EOF
fly ssh sftp put notice.txt /tmp/notice.txt -a illogical-control
hub notice post --db /data/control.db --from /tmp/notice.txt --dry-run
hub notice post --db /data/control.db --from /tmp/notice.txt
```

#### Pre-checks (an hour before)

```
fly status -a illogical-control; fly status -a arugula-platform   # the release's image, checks passing
fly volumes list -a arugula-platform     # platform_data, attached to M1
fly ips list -a arugula-platform         # nothing yet
fly mpg status 1zqyxr76dp1owp8m          # ready
plat migrations --db /data/platform.db   # 1–4, "nothing to run"
put_url
# A trial copy into the real cluster, timed. Nothing reads Postgres yet. It
# migrates the empty cluster to this build (1–4) and leaves rows there,
# which the window's copy replaces (--replace).
time plat copy --from /data/platform.db --to @/tmp/pg.url | tee trial.txt
plat migrations --db @/tmp/pg.url        # 1–4 on Postgres
plat secrets-check --db @/tmp/pg.url     # "N open" with the platform's key
```

The trial's time is the window's copy. Take the latency baseline too (the
first window's curl loop on `/api/chat`).

#### The window

1. **Maintenance on.** Platform routes answer 503 within a second.

   ```
   hub maintenance on --retry-after 60 --db /data/control.db
   curl -si https://control.arugula.io/api/chat | head -4     # 503, Retry-After: 60
   ```

2. **Pause the platform** (one restart), then let a tick in flight finish:

   ```
   fly secrets set -a arugula-platform ARUGULA_PLATFORM_PAUSED=1
   fly logs -a arugula-platform --no-tail | grep 'paused: no cloud worker'
   sleep 30
   ```

3. **Portable checksums of the file.** The restart cleared `/tmp`, so put
   the URL back:

   ```
   put_url
   plat checksums --db /data/platform.db --portable | tee before.txt
   ```

4. **Copy into Postgres**, replacing the trial's rows:

   ```
   time plat copy --from /data/platform.db --to @/tmp/pg.url --replace | tee copy.txt
   diff <(sums before.txt | cut16) <(copied copy.txt) && echo same
   ```

   It ends `23 tables, N rows: every count and checksum matches`.

5. **Both sides equal `before.txt`, and the secrets open.** Postgres holds
   the rows, and nothing has written the file since:

   ```
   plat checksums --db @/tmp/pg.url | tee pg.txt
   plat checksums --db /data/platform.db --portable | tee file-after.txt
   diff <(sums before.txt) <(sums pg.txt) && diff <(sums before.txt) <(sums file-after.txt) && echo both
   plat secrets-check --db @/tmp/pg.url
   ```

6. **The flip:** the URL from Infisical as a secret, which is one restart
   onto Postgres, still paused. Nothing is printed:

   ```
   printf 'ARUGULA_PLATFORM_DATABASE_URL=%s\n' "$(infisical secrets get ARUGULA_PLATFORM_DATABASE_URL --env prod --path /illogical/control --plain --silent)" \
     | fly secrets import -a arugula-platform
   fly logs -a arugula-platform --no-tail | grep -E 'this machine leads|schema|refused'
   fly checks list -a arugula-platform      # passing
   plat checksums --db env:ARUGULA_PLATFORM_DATABASE_URL | tee pg-flipped.txt
   diff <(sums before.txt) <(sums pg-flipped.txt) && echo still
   ```

   The logs should say `this machine leads` and no `the platform's schema,
   on Postgres` line (that appears only when a migration runs, and the copy
   already ran them).

7. **Unpause** (one restart). It shouldn't say `paused`:

   ```
   fly secrets unset -a arugula-platform ARUGULA_PLATFORM_PAUSED
   fly logs -a arugula-platform --no-tail | grep -E 'Arugula platform|this machine leads|paused|refused'
   ```

8. **Maintenance off:**

   ```
   hub maintenance off --db /data/control.db
   curl -si https://control.arugula.io/api/chat | head -1     # 401: answered, by the platform
   ```

#### Post-checks

These are the first window's: chat lists, a room reads, search, post;
a Rebuild or a new workspace naming a secret; a Cloud workspace turn; an
@-mention push to a phone; the MCP `cloud` tool; logs clean; latency. One
thing is new: the platform no longer runs Litestream (`backing up with
Litestream` is gone from its start), because on Postgres the nightly dump
backs it up.

#### Two machines

9. **A private IPv6 for Flycast** (never a public one):

   ```
   fly ips allocate-v6 --private -a arugula-platform
   fly ips list -a arugula-platform         # only a private_v6
   ```

10. **Scale to two in iad.** Fly offers to make the second machine's
    volume: say yes. It's empty, 1 GB, about $0.15 a month, and it's only
    there because `[[mounts]]` is in `fly.toml`:

    ```
    fly scale count 2 -a arugula-platform --region iad
    fly status -a arugula-platform           # two app machines, started, checks passing
    fly logs -a arugula-platform --no-tail | grep -c 'this machine leads'   # 1
    M2=$(fly machine list -a arugula-platform --json | jq -r --arg m "$M" '.[] | select(.config.metadata.fly_process_group == "app" and .id != $m) | .id')
    ```

    From here CI deploys both machines, rolling. `ci-platform-deploy`
    allows two because `ARUGULA_PLATFORM_DATABASE_URL` is set.

11. **The hub through Flycast** (one hub restart; relay sockets reconnect
    by themselves):

    ```
    fly secrets set -a illogical-control ARUGULA_PLATFORM=http://arugula-platform.flycast:7700
    fly logs -a illogical-control --no-tail | grep -E 'runs out of process|the platform can.t be reached'
    ```

12. **Both machines serve.**
    1. Each machine's `/platform.json`, through a WireGuard proxy to that
       machine: `database` true on both, `leader` true on exactly one.

       ```
       for m in $M $M2; do
         fly proxy 17700:7700 "$m.vm.arugula-platform.internal" -a arugula-platform & p=$!; sleep 3
         curl -s localhost:17700/platform.json; echo; kill $p; wait $p 2>/dev/null
       done
       ```

    2. Fan-out through the real gateway. Use a channel you're in (`ROOM`,
       from its URL) and the page's cookie. A live stream holds one
       connection, and Flycast sends new connections to the less busy
       machine, so the posts usually land on the other machine and reach
       the stream over `LISTEN/NOTIFY`:

       ```
       COOKIE='session=…'; ROOM=ch_…
       H=(-H "cookie: $COOKIE" -H 'origin: https://control.arugula.io')
       curl -sN "${H[@]}" https://control.arugula.io/api/chat/live > live.txt & L=$!; sleep 2
       for i in 1 2 3 4 5 6; do
         curl -s "${H[@]}" -H 'content-type: application/json' -d "{\"text\":\"M145 fan-out $i\"}" \
           "https://control.arugula.io/api/chat/rooms/$ROOM/messages" | jq -r .seq
       done
       sleep 2; grep -c "\"type\":\"message\",\"room\":\"$ROOM\"" live.txt   # 6
       kill $L
       ```

    3. Each machine alone. Stop one, run (2) with two posts, start it
       again, and wait for its checks before doing the other. Don't do
       this during a deploy.

       ```
       fly machine stop $M -a arugula-platform     # then (2); then:
       fly machine start $M -a arugula-platform && fly checks list -a arugula-platform
       fly machine stop $M2 -a arugula-platform    # then (2); then:
       fly machine start $M2 -a arugula-platform && fly checks list -a arugula-platform
       ```

13. **The first nightly dump, by hand, and the drill against it.** Make the
    dump machine ("The nightly dump", below: `fly machine run …`), start
    it, wait for its `platform-dump: ok` line, then run
    `scripts/platform-restore-drill` on geek. It should end `drill: …
    every count and checksum equals the dump's record`.

#### Rolling back

At each point, from where it went wrong. `M` is the machine with the
original volume (M1). Always pause before the platform changes database:
two cloud workers on two databases would both drive Managoat.

- **Before step 6** (no flip yet): `fly secrets unset -a arugula-platform
  ARUGULA_PLATFORM_PAUSED`, then `hub maintenance off --db
  /data/control.db`. The copy only read the file. Postgres keeps a copy,
  which the next attempt replaces.
- **Step 6 went wrong** (the platform doesn't start on Postgres, or its
  checks fail): put it back on the file while it's still paused, check the
  file, and let it go:

  ```
  fly secrets unset -a arugula-platform ARUGULA_PLATFORM_DATABASE_URL     # restart, on the file
  plat checksums --db /data/platform.db --portable > file-back.txt
  diff <(sums before.txt) <(sums file-back.txt) && echo untouched
  fly secrets unset -a arugula-platform ARUGULA_PLATFORM_PAUSED
  hub maintenance off --db /data/control.db
  ```

- **After step 7, before step 8** (the platform ran on Postgres and nobody
  wrote): pause, unset the URL, unpause, maintenance off. The platform's
  own background writes on Postgres (feed positions, Managoat events) are
  lost, and redone from the file's older cursors.
- **After step 8** (people wrote on Postgres): copy Postgres back over the
  file first. If step 11 ran, put the hub back on `.internal` first (below).
  If step 10 ran, go back to the one original machine first.

  ```
  hub maintenance on --retry-after 60 --db /data/control.db
  fly secrets set -a arugula-platform ARUGULA_PLATFORM_PAUSED=1           # both machines restart
  fly machine destroy $M2 -a arugula-platform --force                     # if step 10 ran; keep M
  plat copy --from env:ARUGULA_PLATFORM_DATABASE_URL --to /data/back.db | tee back.txt
  plat copy --from env:ARUGULA_PLATFORM_DATABASE_URL --to /data/platform.db --replace | tee back-file.txt
  diff <(copied back.txt) <(copied back-file.txt) && echo same
  fly secrets unset -a arugula-platform ARUGULA_PLATFORM_DATABASE_URL     # on the file, still paused
  fly secrets unset -a arugula-platform ARUGULA_PLATFORM_PAUSED
  hub maintenance off --db /data/control.db
  ```

  `fly volumes list` then shows M2's volume unattached: destroy it
  (`fly volumes destroy <id>`). Litestream resumes under `platform` when
  the platform starts on its file. Destroy the dump machine too, if it
  was made (`fly machine destroy <id> --force`).
- **Step 11 went wrong, or before any rollback after it:** put the hub
  back on the private network's direct address (one hub restart):
  `fly secrets set -a illogical-control ARUGULA_PLATFORM=http://arugula-platform.internal:7700`.
  The private IPv6 can stay, since only the private network reaches it.
- **Step 12 shows a machine that doesn't serve:** `fly machine destroy` it
  (never M) and scale again. One machine on Postgres is a working
  platform.

#### The 14 days after

`/data/platform.db` on M's volume is the rollback for 14 days. Nothing
writes it any more, and its portable checksums stay `before.txt`'s. To see
that now and then:

```
M=<M1's id> plat checksums --db /data/platform.db --portable > later.txt
diff <(sums before.txt) <(sums later.txt) && echo untouched
```

Never destroy M1's volume in those 14 days, and never scale down by
destroying M1. A rollback after the 14 days restores a nightly dump instead
("Restoring the platform's Postgres"). After the 14 days a follow-up deletes
the file and the volumes (filed from M145's PR).

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
`platform.db` while it runs on that file (on Postgres, from M145, "The
nightly dump" below backs it up instead), the same way (`arugula_control_common::backup`): each with
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

### The nightly dump (M145)

On Postgres, Litestream has nothing to back up: the platform runs it only on
its file. Fly Managed Postgres keeps its own backups (`fly mpg backup list`,
restored into a new cluster with `fly mpg restore`), but those live with
Fly. The nightly dump is the platform's copy outside Fly: a `pg_dump`,
encrypted, in R2, kept 30 days.

**Where it runs:** a scheduled Fly machine in the `arugula-platform` app
(`fly machine run --schedule daily`). It isn't one of the app's process
groups, so `fly deploy`, `fly scale` and `ci-platform-deploy`'s checks
leave it alone (they count only `fly_process_group == "app"`). It has no
service, so Flycast never sends it a call. It skips DNS registration, so
`arugula-platform.internal` never names it. It's found by its metadata,
`role=platform-dump`.
- **The image** is `packaging/platform-dump`: Postgres's own
  `postgres:17` image for `pg_dump` 17 (at least the server's major
  version), plus `age`, `curl` and `jq`, plus the **same `arugula-platform`
  binary the platform runs**.
- **Why the same binary:** the migration check and the checksums are that
  build's, so they read the tables the platform has. After each platform
  deploy, `scripts/ci-platform-deploy dump` builds the image from the
  release's binary, pushes it as `registry.fly.io/arugula-platform:dump-vX.Y.Z`,
  and moves the dump machine onto it with `--skip-start`. It never fails a
  deploy. If the two builds ever differ, the dump says so in its log and
  runs on.
- **Considered and not chosen:**
  - *A separate app:* fewer secrets on the machine, but its image would
    need its own deploy token in CI (a workflow change) or would drift
    behind the platform.
  - *A leader-only task in the platform:* the serving image is distroless
    and has no `pg_dump`, and a dump bug would ship with every platform
    deploy.
  - *A GitHub Actions cron:* GitHub would need a WireGuard way into 6PN,
    the database URL and an org token, and the data would pass through
    GitHub's runner.

**What it does** (`packaging/platform-dump/dump.sh`, once a day, then it
exits):
1. **The migration check:** `arugula-platform migrations` on the database,
   read-only. A database this build would migrate fails the run.
2. **One snapshot:** a `psql` session exports a snapshot and holds it
   open. `checksums --snapshot` and `pg_dump --snapshot` both read it, so
   the record is exactly the dump, whatever is written meanwhile.
3. **The dump:** `pg_dump --format=custom --no-owner --no-privileges
   --schema=public | age -r <each recipient>` → `platform-pg/<time>.pgdump.age`.
   It's encrypted to public keys only: no key on Fly can open it.
4. **The record:** `platform-pg/<time>.json`, and the same as
   `platform-pg/latest.json`. It holds the time, the object's size and
   sha256, both builds' versions, `pg_dump`'s and the server's versions,
   the migrations, and each table's rows and portable checksum. It's not
   encrypted, and it holds counts and hashes, no row's content.
5. **A check:** it reads the object's size back from R2.
6. **The prune** (below): the dumps older than 30 days go, never the
   newest 3.
7. **The log:** `platform-dump: ok <object>: N bytes, …, T tables, R rows`
   and `platform-dump: pruned …`, or `platform-dump: FAILED at <step>` and
   a non-zero exit.

**Secrets and settings.**
- **The database:** `ARUGULA_PLATFORM_DATABASE_URL`, the app's own
  secret. `ARUGULA_DUMP_DATABASE_URL`, if set, overrides it (a `reader`
  user of its own: `fly mpg users create 1zqyxr76dp1owp8m -u platform-dump -r reader`).
- **R2:** the app's `LITESTREAM_*` (bucket `illogical-control-backups`,
  an *Object Read & Write* token for that bucket only), or
  `ARUGULA_DUMP_BUCKET`, `ARUGULA_DUMP_ENDPOINT`,
  `ARUGULA_DUMP_ACCESS_KEY_ID` and `ARUGULA_DUMP_SECRET_ACCESS_KEY` for a
  token of its own.
- **The recipients:** `ARUGULA_DUMP_RECIPIENTS`, the machine's env (age
  public keys aren't secret). It can take more than one.
- **Optional:** `ARUGULA_DUMP_PING_URL` (below), `ARUGULA_DUMP_PREFIX`
  (`platform-pg`), `ARUGULA_DUMP_KEEP_DAYS` (30) and
  `ARUGULA_DUMP_KEEP_NEWEST` (3).
- **Decided (lead, 2026-10-21):** R2 through the app's Litestream token,
  the database as `fly-user` through the app's URL, and one age key, in
  Infisical. The overrides above stay for if that changes.

**Kept 30 days, by the job itself.** There's no R2 lifecycle rule: setting
one needs Cloudflare access, which nobody operating this has. After a
successful upload, the job lists `platform-pg/` (S3 ListObjectsV2, with the
same SigV4 `curl` it puts with) and deletes each dump and its record whose
time, in its name, is more than 30 days ago. It **never deletes the newest
3**, whatever their age, so a stopped clock or a run of failures can't
empty the prefix. `latest.json` and anything not named like a dump are
left alone. A prune that fails logs `platform-dump: warning: pruning old
dumps failed` and the run still exits 0: its dump is in, and the next run
prunes. This needs list and delete on the bucket, which the Litestream
token (*Object Read & Write*) has.

**Noticing a failed or missing dump.** Nothing in this repository alerts
today: "A spend alert" is Fly's billing, and home-cloud's Signal alerts
watch the house. So, in order of effort:
- **Every run** ends with `platform-dump: ok` or `platform-dump: FAILED at
  …`, and exits non-zero on failure. See
  `fly logs -a arugula-platform --machine <dump id> --no-tail`, and the
  exit code in `fly machine status <dump id> -a arugula-platform`.
- **The newest dump's age:** `scripts/platform-restore-drill --age` reads
  `latest.json` and exits 1 past 26 hours.
- **Decided (lead, 2026-10-21): those two, checked by hand.** Run
  `scripts/platform-restore-drill --age` now and then, and after anything
  touching the platform's app.
- **If that changes:** a dead man's switch, which also notices a run that
  never happened. Set `ARUGULA_DUMP_PING_URL` on the machine. It follows
  healthchecks.io's convention: `<url>/start`, then `<url>` on success, or
  `<url>/fail` with the failing step.

**Making it** (once, after the window's flip; the lead's steps):

1. **The age keypair**, made locally. The private key goes only into
   Infisical:

   ```
   umask 077
   age-keygen -o platform-dump.key 2>/dev/null
   age-keygen -y platform-dump.key > platform-dump.pub       # age1…, not secret
   infisical secrets set --env prod --path /illogical/control \
     "ARUGULA_PLATFORM_DUMP_AGE_KEY=$(grep AGE-SECRET-KEY platform-dump.key)" >/dev/null
   # Round trip: Infisical's key gives the same public key.
   infisical secrets get ARUGULA_PLATFORM_DUMP_AGE_KEY --env prod --path /illogical/control --plain --silent \
     | age-keygen -y | diff - platform-dump.pub && echo same
   shred -u platform-dump.key
   ```

2. **The platform's Litestream keys into Infisical,** for the drill: it
   reads R2 with them. Fly can't read a secret back, so take them from the
   platform's running process, as the first window took the hub's. Python
   sets them, then checks the round trip, and prints only names:

   ```
   pid=$(plat secrets-check --db env:ARUGULA_PLATFORM_DATABASE_URL | sed -n 's/.*(pid \([0-9]*\)).*/\1/p')
   fly ssh sftp get /proc/$pid/environ plat.environ -a arugula-platform ${M:+--machine $M}
   python3 -I - plat.environ <<'PY'
   import subprocess, sys
   want = ["LITESTREAM_ACCESS_KEY_ID", "LITESTREAM_SECRET_ACCESS_KEY", "LITESTREAM_ENDPOINT", "LITESTREAM_BUCKET"]
   raw = open(sys.argv[1], "rb").read()
   if not raw:
       sys.exit("plat.environ is empty: sftp read /proc as 0 bytes. Don't guess values; stop and ask.")
   env = dict(kv.split("=", 1) for kv in raw.decode().split("\0") if "=" in kv)
   missing = [k for k in want if not env.get(k)]
   if missing:
       sys.exit("not in the platform's environment: " + ", ".join(missing))
   where = ["--env", "prod", "--path", "/illogical/control"]
   subprocess.run(["infisical", "secrets", "set", *where, *[f"{k}={env[k]}" for k in want]], check=True, stdout=subprocess.DEVNULL)
   for k in want:
       got = subprocess.run(["infisical", "secrets", "get", k, *where, "--plain", "--silent"], capture_output=True, text=True).stdout
       print(k, "same" if got.strip() == env[k] else "DIFFERS")
   PY
   shred -u plat.environ
   ```

3. **No bucket rule:** the job prunes its own dumps (above).
4. **The machine,** on the image the release pushed (`ci-platform-deploy`
   pushes `dump-vX.Y.Z` even before the machine exists):

   ```
   fly machine run registry.fly.io/arugula-platform:dump-vX.Y.Z -a arugula-platform \
     --name platform-dump --region iad --schedule daily --restart no \
     --vm-size shared-cpu-1x --vm-memory 512 --skip-dns-registration \
     --metadata role=platform-dump \
     --env ARUGULA_DUMP_RECIPIENTS="$(cat platform-dump.pub)"
   # add --env ARUGULA_DUMP_PING_URL=https://hc-ping.com/<uuid> for the dead man's switch
   D=$(fly machine list -a arugula-platform --json | jq -r '.[] | select(.config.metadata.role == "platform-dump") | .id')
   fly status -a arugula-platform     # still two app machines; the dump machine is beside them
   ```

5. **The first run, by hand:**

   ```
   fly machine start $D -a arugula-platform
   fly logs -a arugula-platform --machine $D --no-tail | grep platform-dump    # platform-dump: ok …
   scripts/platform-restore-drill                                               # on geek: the drill, below
   ```

**What it costs:** a shared-cpu-1x for under a minute a day, which is
cents. In R2, a dump of today's data is tens of KB; at R2's $0.015 a
GB-month, 30 of them cost nothing to speak of.

### Restoring the platform's Postgres

**The drill** (`scripts/platform-restore-drill`, on geek, never against
production) restores the newest dump into a scratch `postgres:17` container
and checks it against the dump's record:
1. It reads `latest.json`, downloads the object, and checks its sha256.
   It reaches R2 with the keys the dump uses, from Infisical (`arugula`
   prod, `/illogical/control`): `LITESTREAM_ENDPOINT`, `LITESTREAM_BUCKET`,
   `LITESTREAM_ACCESS_KEY_ID` and `LITESTREAM_SECRET_ACCESS_KEY`, escrowed
   by "Making it" step 2. If a token of its own is ever made,
   `ARUGULA_DUMP_*` names there come first.
2. It decrypts it with `ARUGULA_PLATFORM_DUMP_AGE_KEY` from Infisical,
   through a pipe, so the key is never written to disk.
3. It runs `pg_restore --clean --if-exists` into the container.
4. It runs `arugula-platform migrations` on it and compares the result
   with the record's migrations.
5. It runs `arugula-platform checksums` on it and compares every table's
   rows and checksum with the record's. The record was read in the dump's
   own snapshot, so they have to be equal.
6. `--secrets` also opens every cloud secret with
   `ARUGULA_CLOUD_SECRET_KEY` from Infisical.

```
just static                                   # at the dump's release, or pass --bin
scripts/platform-restore-drill                # the newest; --object platform-pg/<time>.pgdump.age for another
scripts/platform-restore-drill --secrets      # and every cloud secret opens
scripts/platform-restore-drill --age          # only the newest dump's age; exit 1 past 26 h
```

It ends `drill: <object> (taken <time>) restored in N s: T tables, R rows,
every count and checksum equals the dump's record; migrations up to V
match`. Otherwise it prints `DIFFERS` beside each table that differs and
exits 1. Check with the build that made the dump (the record's `platform`):
a table whose columns changed between builds would differ. Run it once a
month or so, and after any change to the dump.

**Rehearsed** with `testnet/window2.sh` (an S3 store standing in for R2):
the dump made by hand, the drill passing, and a drill against a record that
lies about one table failing with that table's `DIFFERS`. Also the prune:
old dumps seeded at 35, 40, 45 and 60 days, the next run deleting all but
the 35-day one (kept as one of the newest 3), and a run whose listing is
refused logging the failed prune and exiting 0 with its dump in.

**For real, after losing the cluster or its data:**
- **First choice: Fly's own backups.** `fly mpg backup list
  1zqyxr76dp1owp8m`, then `fly mpg restore 1zqyxr76dp1owp8m
  --backup-id … --name arugula-platform-db-restored` (or `--pitr-time`).
  This makes a new cluster: put its direct URL in Infisical and set
  `ARUGULA_PLATFORM_DATABASE_URL` as in the window's step 6, with the
  platform paused and maintenance on.
- **Without Fly's backups:** use the nightly dump. Pause the platform and
  turn maintenance on. Make or choose an empty database, decrypt the dump
  as the drill does, then `pg_restore --no-owner --no-privileges --clean
  --if-exists --exit-on-error -d <url> dump.pgdump`, from a machine on
  6PN or through `fly mpg proxy`. Check it with `checksums` against the
  record, point the platform at it, unpause, and turn maintenance off.
  Anything written after the dump is lost.

### The Postgres bill

Fly's pricing page (<https://docs.fly.io/about/pricing>, checked
2026-10-21): Managed Postgres **Basic is $38/month**, and storage is
**$0.28/GB/month**. `arugula-platform-db` is Basic with 10 GB, so
$38 + $2.80 is about **$40.80/month**. The plan includes Fly's backups,
pooling and failover. The rest of M145 adds:
- the second platform machine (shared-cpu-1x, 512 MB, about $3 a month
  running all the time);
- its empty 1 GB volume (about $0.15, until the 14-day follow-up drops the
  volumes);
- the dump machine (cents);
- the dumps in R2 (cents).

So hosted control's platform runs about $45 a month above the hub. The
pricing page's other figure is Performance at $1,922/month. Fly's
Managed Postgres page has the plans in between; Basic is the one to stay
on until the platform's data or connections outgrow its 1 GB of memory.

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

**The platform on Postgres (M143).** `ARUGULA_PLATFORM_DATABASE_URL`
(`--platform-database-url`), a `postgres://` URL, puts the platform's
tables in Postgres instead of control's file; unset, nothing changes. It
runs `crates/platform/migrations/postgres/` the same way, in one
transaction under an advisory lock, and logs `the platform's schema, on
Postgres` with what it ran; there's nothing to adopt, since Postgres starts
empty and M145 copies the rows in (`arugula-platform copy --from
/data/platform.db --to postgres://…`, with portable checksums that
`checksums --db postgres://…` and `checksums --db FILE --portable` print
alike). Point it at the cluster's direct address
(`direct.<id>.flympg.net`), not the PgBouncer one: the platform keeps
prepared statements per connection, takes advisory locks and `LISTEN`s
(M144), which a pooler in transaction mode breaks. It speaks plain TCP over the private network
(add `?sslmode=disable` if the URL asks for TLS). Text is `COLLATE
pg_c_utf8`, which needs Postgres 17. Hosted control doesn't use it until
M145's window.

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
  WHERE project_id IS NOT NULL AND NOT (kind = 'channel' AND scope LIKE 'team:%')
  AND NOT (kind = 'object' AND scope = 'project:' || project_id);
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
