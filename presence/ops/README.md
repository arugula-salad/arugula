# Running arugula.stream's relay

How the hosted relay is kept safe and watched (#318's P12): its abuse
limits, its metrics and alerts, its backups, and how much load it holds.
Everything here is relative to `presence/`; commands run from there.

## Abuse limits

`PresenceRelay.Limits` (relay/lib/presence_relay/limits.ex) and
PROTOCOL.md's "Abuse limits". The numbers live in
`relay/config/config.exs`:

| limit | per | default | over it |
|---|---|---|---|
| `socket_connect` | client IP | 120 a minute | the WebSocket upgrade is a 429 with `Retry-After` |
| `email_ip` | client IP | 10 an hour | `POST /api/v1/auth/email` is a 429 |
| `email_address` | address | 5 an hour | no mail, and the answer still says `{sent: true}` |
| `meet_room` | client IP | 20 an hour | `POST /api/v1/meet/rooms` is a 429 |
| `api_write` | client IP | 120 a minute | any POST/PUT/PATCH/DELETE under `/api/v1` is a 429 |
| `demo_token` | client IP | 120 an hour | `POST /api/demo/token` is a 429 |
| app connections | app | 2,000 at once | the join is refused: "this app has too many connections" |

Change them without a deploy of code, as Fly secrets or `[env]`:

- `RATE_LIMITS="socket_connect=600/60,demo_token=off"`: any of the
  per-IP limits, `name=hits/seconds` or `name=off`.
- `APP_MAX_CONNECTIONS=5000`: every app's cap; `APP_CONNECTIONS="meet:10000,app_x:off"`:
  one app's.

How they count:

- **Per node.** Each machine counts in its own ETS table (fixed windows,
  one atomic counter a key), so a client spread over N machines could
  get N times a limit. Fly's proxy mostly sends a client to the same
  nearby machine, and even N× is a ceiling an abuser meets at once.
- **The app cap is cluster-wide**, from a `:pg` group per app, so as
  eventually consistent as the room caps: two machines admitting the
  last connection at the same instant could both let it in.
- **The client IP** is `Fly-Client-IP` on Fly (trusted only when
  `FLY_APP_NAME` is set, which Fly sets), otherwise the peer address. The
  socket can't see headers in `connect/3`, so the endpoint notes the IP
  first (`PresenceRelayWeb.ClientIP`).

## Metrics

`GET /metrics` on port 9091 (`METRICS_PORT`), Prometheus' text format,
from a small exporter of our own (`PresenceRelay.Metrics`: no new deps;
what it counts already arrives as telemetry, and the gauges are read at
scrape time). No Fly service exposes 9091; Fly's `[metrics]` section in
`fly.toml` and `fly.staging.toml` has Fly scrape it into its managed
Prometheus, labelled `app`, `instance` and `region`. See it
in Grafana at https://fly-metrics.net (Fly dashboard → Metrics).

| metric | what |
|---|---|
| `presence_connections` | this machine's room connections (sum across instances) |
| `presence_rooms`, `presence_members` | the cluster's (every node sees the same: take the max) |
| `presence_frames_total{result}` | frames in: `relayed`, `too_big`, `too_soon` (`rate()`: frames/s) |
| `presence_frame_bytes_total` | bytes relayed, before fan-out |
| `presence_messages_total{kind,result}` | say, react, wave, answer; kept or dropped |
| `presence_socket_connects_total{result}` | sockets admitted or refused |
| `presence_joins_total`, `presence_joins_refused_total{reason}` | `token`, `origin`, `guest`, `rate`, `cap`, `room_full`, `connections`, `app_connections`, `wrong_room` |
| `presence_calls_active`, `presence_call_participants` | calls now (cluster) |
| `presence_calls_started_total`, `presence_call_seconds_total` | calls begun; person-seconds in calls (÷ 60: call minutes) |
| `presence_rate_limited_total{limit}` | hits over an abuse limit |
| `presence_http_requests_total{status}`, `presence_http_request_seconds` | HTTP by status class; latency histogram |
| `presence_db_query_seconds`, `presence_db_queue_seconds` | Postgres query time; time waiting for a pooled connection |
| `presence_db_pool_size`, `_ready`, `_queue` | the pool now |
| `presence_usage_unflushed_seconds`, `presence_usage_kept`, `presence_usage_flush_seconds` | usage metering's buffer: seconds since a flush left nothing behind (grows while Postgres refuses writes), what it kept, flush time |
| `presence_cluster_nodes` | nodes this one sees |
| `presence_beam_scheduler_utilization` | the share of the schedulers' time spent working (real CPU, without spin) |
| `presence_beam_memory_bytes{kind}`, `_processes`, `_ports`, `_atoms`, `_run_queue`, `_schedulers`, `_reductions_total`, `_uptime_seconds` | the BEAM's basics |

**The dashboard**: `ops/grafana/relay.json`. In fly-metrics.net:
Dashboards → New → Import → upload the file → pick the Prometheus data
source → Import. Its `app` picker switches between `arugula-stream` and
`arugula-stream-staging`.

## Logs

In prod each line is one JSON object (`PresenceRelay.LogFormatter`):
`time`, `level`, `msg`, `source`, and `request_id` and `app` where there
are ones. Messages are scrubbed of email addresses, JWTs, publishable and
secret keys, the magic link's token, credential query parameters, bearer
tokens and database passwords; Phoenix's own lines filter the same
parameters. Search them with `fly logs -a arugula-stream | grep '"level":"error"'`.
`LOG_LEVEL=warning` quiets the request lines.

## Health and alerts

- `GET /healthz`: up (Fly's machine check, every 15 s; a dead machine
  is restarted and taken out of routing).
- `GET /healthz?db=1`: Postgres answers (P1's).
- `GET /healthz?deep=1`: JSON of every deep check (Postgres within 2 s;
  usage flushed clean within 2 minutes), 503 if any fails.

**The watchdog** (`PresenceRelay.Watchdog`) is the alert path: every
minute the cluster's first node asks every node for its deep health, and
after 3 failures in a row mails `hello@arugula.stream` (`ALERT_EMAIL`)
through the relay's own Resend mailer, again every 6 hours while it
lasts, and once when all's well. No new service, nothing to click,
**as long as** the app has `RESEND_API_KEY` (and `MAIL_FROM` on a domain
Resend has verified) and someone reads hello@arugula.stream. Without a
key it only logs `watchdog: …` warnings.

What it can't see is the whole app down. For that, one Grafana alert
(Jake, once, in fly-metrics.net):

1. Alerting → Contact points → New: type Email, address
   hello@arugula.stream. (If Fly's Grafana won't send email, skip this
   alert: Fly restarts dead machines, and an outage of every machine
   shows on Fly's own status emails for the app.)
2. Alerting → Alert rules → New: query
   `count(up{app="arugula-stream"} == 1) or vector(0)`, condition
   "is below 1", evaluate every 1m for 5m, labels `severity=page`,
   contact point the one above. Save.

## Backups

### Managed Postgres's own

The database is the Managed Postgres cluster `arugula-stream-db`
(id `nlkxjo5pdkd0y93v`, iad). Fly backs every MPG cluster up and keeps a
point-in-time-recovery (PITR) window; restores go **into a new cluster**
(the source is never touched), billed as a cluster of its own.

```sh
fly mpg backup list nlkxjo5pdkd0y93v --all     # full / incr / diff backups, with times and sizes
fly mpg backup create nlkxjo5pdkd0y93v         # a full backup now (before a risky migration)
fly mpg restore nlkxjo5pdkd0y93v --backup-id <id> --name arugula-stream-db-restore
fly mpg restore nlkxjo5pdkd0y93v --pitr-time 2026-11-02T14:05:00Z --name arugula-stream-db-pitr
fly mpg status <new cluster id>                # creating → initializing → ready
```

Fly's docs don't state the PITR window or how long backups are kept:
`fly mpg backup list --all` shows the oldest backup, and a `--pitr-time`
outside the window is refused (HTTP 422), which is how to find its edge.

**A real restore** (data lost or corrupted). Our data is small, so the
time is mostly the new cluster provisioning; nobody has timed one yet
(a guess: half an hour end to end), so time the first and write it here:

1. Pick the moment: just before the damage (`fly logs`, the deploy
   time, `presence_db_*` in Grafana).
2. `fly mpg restore … --pitr-time <moment>` (or `--backup-id`); wait for
   `ready` in `fly mpg status`.
3. Check the copy with the drill below, `SOURCE_URL` pointed at the new
   cluster through `fly mpg proxy <new id>`: it proves the dump/restore
   path and shows the row counts to eyeball.
4. Switch the app: `fly mpg detach <old id> -a arugula-stream`, then
   `fly mpg attach <new id> -a arugula-stream` (sets `DATABASE_URL`;
   the machines restart, and `release_command` runs migrations).
5. `curl https://arugula.stream/healthz?deep=1` → `"ok": true`.
6. What's lost: writes after the restore point (sign-ups, apps, keys,
   messages, a few minutes of usage). Usage that's still in a node's
   buffer is written to whichever database the node has at its next
   flush.
7. Keep the old cluster a few days for anything to copy back, then
   `fly mpg destroy <old id>`.

### The drill: `ops/backup-drill.sh`

Proves we can get the data out and back: pg_dump the database, restore
it into a scratch database, compare, drop the scratch database. The
script's header lists its variables. The source is only read, in one
exported snapshot that pg_dump dumps too, so a live database can't make
a false difference. It checks: the same tables; each table's row count
and a checksum of all its rows; the same constraints and indexes; every
sequence at or past its column's maximum; the last migration; no orphan
rows; usage days agreeing with usage members. Exit 0 when all pass.

Locally (done for P12, 2026-10-29: 13 tables, ~31k rows, all checks
passed; ~1 s each way):

```sh
psql postgres://postgres:postgres@127.0.0.1:55432/presence_dev -f ops/drill-seed.sql   # made-up data
SOURCE_URL=postgres://postgres:postgres@127.0.0.1:55432/presence_dev \
SCRATCH_ADMIN_URL=postgres://postgres:postgres@127.0.0.1:55432/postgres \
  ops/backup-drill.sh
```

**Against staging** (nothing is written to staging; the scratch copy is
a throwaway local container):

```sh
# 1. A scratch Postgres on this machine.
docker run -d --rm --name presence-drill-pg -e POSTGRES_PASSWORD=drill -p 127.0.0.1:55499:5432 postgres:17
# 2. Staging's database on localhost:16380 (leave it running).
fly mpg proxy nlkxjo5pdkd0y93v &
# 3. Staging's credentials, from its DATABASE_URL (don't paste it anywhere):
fly ssh console -a arugula-stream-staging -C 'printenv DATABASE_URL'
#    → postgres://USER:PASSWORD@HOST/presence_staging?…: keep USER and PASSWORD.
# 4. The drill (PG_DOCKER: the tools match the server's major version).
SOURCE_URL='postgres://USER:PASSWORD@127.0.0.1:16380/presence_staging?sslmode=disable' \
SCRATCH_ADMIN_URL=postgres://postgres:drill@127.0.0.1:55499/postgres \
PG_DOCKER=postgres:17 \
  ops/backup-drill.sh
# 5. Clean up.
kill %1; docker stop presence-drill-pg
```

If the proxy goes through PgBouncer and the snapshot can't be exported
(the drill stops at "couldn't export a snapshot"), run step 4 against
the cluster's direct address instead; `fly mpg status` lists it. Run the
drill every quarter, and after any change to the schema or to Fly's
Postgres.

## Load

`ops/load/probe.mjs` (Node 24, no packages): N rooms × M members as real
WebSocket clients speaking the protocol. Each sends a rig-sized frame
(32 bytes) every 83 ms (the client's 12 frames a second; `--gap 70` is
the relay's floor), a text message about every 20 s, a reaction every
10 s and a wave every 30 s, and measures fan-out latency (sent → heard by
each other member: p50/p95/p99/max), frames missing (gaps in each
sender's sequence numbers), say round trips, and errors. `--help` lists
the options; `--metrics` reads a local relay's /metrics before and after,
to tell frames the relay dropped as too soon from frames lost.

Team scale is 10 rooms × 8; 10× is 100 rooms × 8 (800 clients).

### Local results (P12, 2026-10-29)

On a 32-thread workstation, the relay in prod mode, the probe on the
same machine, Postgres shared (pool of 4):

| load | frames in/out per s | fan-out p50 / p95 / p99 | missing | say p99 | relay CPU (busy schedulers) | BEAM memory |
|---|---|---|---|---|---|---|
| 1×: 10 × 8 = 80 | 962 / 6.7k | 0.3 / 0.6 / 1.1 ms | 0 | 7 ms | ~0.2 | ~92 MB |
| 10×: 100 × 8 = 800 | 9.6k / 67k | 0.2 / 0.6 / 0.9 ms | 0.004% (too-soon drops) | 7 ms | ~2.5 | ~340 MB |
| 20×: 200 × 8 = 1,600 | 19k / 135k | 0.3 / 0.8 / 1.4 ms | 0.012% | 6 ms | ~5.3 | ~510 MB |
| 40×: 400 × 8 = 3,200 | 38k / 269k | 0.3 / 1.4 / 8.6 ms | 0.39% (too-soon drops) | 20 ms | ~13 | ~1.07 GB |

With one scheduler (`ERL_FLAGS="+S 1:1"`, roughly one dedicated vCPU):

| load | busy | fan-out p50 / p95 / p99 | missing | BEAM memory |
|---|---|---|---|---|
| 80 | 8% | 0.3 / 0.7 / 1.1 ms | 0 | 92 MB |
| 400 | 40% | 0.9 / 2.9 / 4.0 ms | 0 | 185 MB |
| 800 | 90% (saturated) | 9.9 / 54 / 105 ms | 3.7% (too-soon drops from queueing) | 311 MB |

Every "missing" frame above matches the relay's own too-soon drops (two
frames arriving under 70 ms apart, from jitter or queueing), so fan-out
itself lost nothing.

What that means for Fly: one vCPU carries about 400–500 clients at 12
frames a second comfortably, and about 250–400 KB of memory a
connection (each socket's and channel's process heap). 10× team scale
across staging's two machines (400 each) should hold on CPU and memory,
**but** a `shared-cpu-1x` is guaranteed only a slice of a core and
bursts above it on credit: a sustained 40% may get throttled. Watch
`presence_beam_scheduler_utilization` and frames too soon; if it
throttles, `shared-cpu-2x` or `performance-1x` (and `memory = "1gb"`
past ~1,000 connections a machine) is the fix.

### Staging, from two regions at once

1. Lift staging's per-IP limits for the run (the probe's clients share
   one IP each), and allow enough connections:

   ```sh
   fly secrets set -a arugula-stream-staging RATE_LIMITS="socket_connect=100000/60,demo_token=100000/3600"
   ```

2. On each machine (say one in the US east, one in Europe), the same
   rooms, half the members each, starting together:

   ```sh
   START=$(date -u -d '+2 min' +%s)000     # agree on it, then on both:
   node ops/load/probe.mjs --url https://arugula-stream-staging.fly.dev \
     --rooms 100 --members 4 --prefix p12-$START --start-at $START \
     --duration 120 --ramp 30 --id us        # --id eu on the other
   ```

   Each prints its own summary. "Same machine" latency is exact;
   "other machines" is by wall clock (check both clocks: `chronyc
   tracking` or `timedatectl`), and includes the hop between the Fly
   regions the two clients landed in.

3. Watch the Grafana dashboard during the run (scheduler utilization,
   memory, frames too soon, pool wait).
4. Put the limits back: `fly secrets unset -a arugula-stream-staging RATE_LIMITS`.

Done when: 10× (100 rooms × 8) holds: every client connected, no
channel errors, frames missing ≈ the relay's too-soon drops, fan-out p99
within a frame (83 ms), and both machines' memory under their limit.
