# Launch runbook

What to watch and what to run while hosted Arugula takes its launch traffic
(part of Arugula #532). It replaces the launch-day section of #75. The background is in [control-ops.md](control-ops.md); this
page has the commands, in the order you'd reach for them.

Hosted control is two Fly apps:

| App | What | Scales by |
|---|---|---|
| `illogical-control` | the hub: sign-in, the relay, control's page; SQLite on a volume, backed up by Litestream to R2 | a bigger machine (one machine only: it holds the relay and the database) |
| `arugula-platform` | chat, Cloud, billing, hosted workspaces; Managed Postgres `arugula-platform-db` | more machines, or bigger ones |
| `arugula-platform` (the `platform-dump` machine) | the nightly encrypted `pg_dump` to R2 | n/a |

Anything private (who's on call and how to reach them, account links) is in
the operator's private notes, not here.

## The 48-hour watch

From the announcement until 48 hours later, someone is always watching.
Shifts are 6 hours, with a backup who can be reached within 15 minutes.
Fill this in the week before.

| Shift (UTC) | Primary | Backup |
|---|---|---|
| Day 1, 13:00–19:00 (the announcement) | | |
| Day 1, 19:00–01:00 | | |
| Day 2, 01:00–07:00 | | |
| Day 2, 07:00–13:00 | | |
| Day 2, 13:00–19:00 | | |
| Day 2, 19:00–01:00 | | |
| Day 3, 01:00–07:00 | | |
| Day 3, 07:00–13:00 | | |

**At the start of each shift:**
1. Open the dashboard ("The dashboard", below) and the status page.
2. Read the last shift's handoff note in the launch thread: what's broken, what
   was changed, which switches are on.
3. `fly status -a illogical-control` and `fly status -a arugula-platform`:
   every machine `started`, checks passing.
4. Skim new issues and Discussions; label and answer what you can.

**At the end:** a handoff note. It says what happened, what you changed and
which switches are on, and what to keep an eye on.

**Wake the backup** when an alert below has been red for 15 minutes and the
first step hasn't helped, or before any restore.

## The dashboard

Grafana at fly-metrics.net (Fly's managed Grafana for the org), dashboard
**Arugula launch**. It's in the repository as
[launch-dashboard.json](launch-dashboard.json): import it (*Dashboards → New
→ Import*), and pick the Prometheus data source. It reads Fly's own metrics
and control's (`/metrics` on the hub and the platform:
[control-ops.md, "Metrics"](control-ops.md#metrics)).

| Panel | Red at | First step |
|---|---|---|
| Relay sockets | 4,500 (the ceiling is 5,000) | Raise the ceiling with the machine ("Scale up: the hub") |
| Hub memory | 880 MB of 1 GB | Scale up the hub |
| Errors (5xx, 15 min) | 5% | `fly logs -a illogical-control`, then `-a arugula-platform`; roll back if it started with a release |
| Sign-ups (24 h) | n/a | Past what the beta can take: the sign-up switches ("Switches") |
| Hosted boxes running | n/a | Watch it against the E2B line in "Spend" |
| Spend this month (est.) | $400 | "Spend", below |

The latency panel is the hub's p95 as Fly's proxy measures it. When it climbs
with sockets and CPU, the hub is the bottleneck; when it climbs alone, look
at the platform and Postgres (`fly mpg status 1zqyxr76dp1owp8m`).

## Scale up

### The hub (`illogical-control`)

The hub is one machine on purpose: its SQLite database and every relay
socket live there. It scales by getting bigger. **A resize restarts it**:
every relayed terminal drops, and daemons and pages reconnect by themselves
within about a minute. Do it between peaks if you can.

```
M=$(fly machine list -a illogical-control --json | jq -r '.[] | select(.config.metadata.fly_process_group == "app") | .id')
fly machine update "$M" -a illogical-control --vm-size shared-cpu-2x --vm-memory 2048 -y
fly status -a illogical-control                     # started, 1/1 checks
```

Rehearsed on a copy of the hub (2026-11-10, the production build on a
throwaway app): shared-cpu-2x with 2 GB, answering again within seconds,
its database untouched. `performance-1x` (2 GB) is the step after that.

To let more sockets in, raise the relay's ceiling with the machine in the
same update. Add
`--env ARUGULA_RELAY_MAX_TOTAL=9000` and keep it below Fly's limits in
`fly.toml` (6,000 soft, 8,000 hard: raise those with a `fly deploy` of the
hub's `fly.toml`). Each idle relay socket costs about 28 KB, so it's open
files and the proxy's limits that bind, not memory
([control-ops.md, "Capacity"](control-ops.md#capacity)).

**Make it stick:** the next release applies the Machine as
`ops/src/fly/control/control.fly.ts` declares it, and that would undo the
resize. Within a day, land a PR that changes it there and in
`packaging/control/fly.toml` (`[[vm]]`) to match.

### The platform (`arugula-platform`)

The platform's machines share Postgres and hold nothing of their own, so it
scales out. A third machine, beside the two:

```
P=$(fly machine list -a arugula-platform --json | jq -r '[.[] | select(.config.metadata.fly_process_group == "app")][0].id')
fly machine clone "$P" -a arugula-platform --region iad
fly status -a arugula-platform                      # three app machines, all passing
```

Flycast spreads the hub's calls across every machine passing its check, so
the new one takes traffic once it's healthy. Bigger, one machine at a time
(rolling, so the other keeps serving):

```
for M in $(fly machine list -a arugula-platform --json | jq -r '.[] | select(.config.metadata.fly_process_group == "app") | .id'); do
  fly machine update "$M" -a arugula-platform --vm-memory 1024 -y
done
```

Rehearsed on a staging copy (2026-11-10): the clone passed its check 6 s
after the command, and a resize took 4 s.

The clone isn't in the release's plan (`ops/src/fly/platform/platform.fly.ts`
declares two). Either add it there, or `fly machine destroy` it once the
peak is over. A resize sticks the same way as the hub's.

### Postgres

`arugula-platform-db` is on Managed Postgres Basic (1 GB). Its plan changes
in Fly's dashboard (*Managed Postgres → arugula-platform-db*), not with
`flyctl`. Look first: `fly mpg status 1zqyxr76dp1owp8m`, and the
dashboard's connections and CPU.

## Roll back

**Fastest: put the previous image back.** Each release records the image it
replaced on the machine (`chant-release-previous-digest`), so going back
needs no build. `fly machine update --image` won't take a bare digest
(`invalid image identifier`), so this goes through the Machines API, as the
release does, keeping the rest of each Machine's config:

```
image_back() {   # APP MACHINE DIGEST
  local t; t=$(fly auth token 2>/dev/null)
  curl -s -H "Authorization: Bearer $t" "https://api.machines.dev/v1/apps/$1/machines/$2" |
    jq --arg img "registry.fly.io/$1@$3" '{config: (.config | .image = $img)}' |
    curl -s -X POST -H "Authorization: Bearer $t" -H 'Content-Type: application/json' \
      "https://api.machines.dev/v1/apps/$1/machines/$2" -d @- | jq -c '{state, image: .config.image, error}'
  curl -s -H "Authorization: Bearer $t" "https://api.machines.dev/v1/apps/$1/machines/$2/wait?state=started&timeout=60" | jq -c .
}
```

The hub:

```
M=$(fly machine list -a illogical-control --json | jq -r '.[] | select(.config.metadata.fly_process_group == "app") | .id')
PREV=$(fly machine list -a illogical-control --json | jq -r '.[] | select(.id == "'"$M"'") | .config.metadata["chant-release-previous-digest"]')
image_back illogical-control "$M" "$PREV"
fly status -a illogical-control                     # 1/1 checks passing
fly machine list -a illogical-control --json | jq -r '.[].config.image'   # the previous digest
```

The platform's two machines, one at a time, each passing its check before
the next. Take the digest from the app machine that carries it (a release
records it on the one it verifies; the `platform-dump` machine's is the
dump image's, not this one).

```
PREV=$(fly machine list -a arugula-platform --json | jq -r '[.[] | select(.config.metadata.fly_process_group == "app") | .config.metadata["chant-release-previous-digest"] // empty][0]')
for M in $(fly machine list -a arugula-platform --json | jq -r '.[] | select(.config.metadata.fly_process_group == "app") | .id'); do
  image_back arugula-platform "$M" "$PREV"
  until fly machine list -a arugula-platform --json | jq -e '.[] | select(.id == "'"$M"'") | all(.checks[]; .status == "passing")' >/dev/null; do sleep 2; done
done
```

Rehearsed (2026-11-10): the hub's commands above, word for word, on a copy
of the hub running the current production build. They took it back to the
release before, by its digest, in 5 s, database intact. The same on a staging
copy of the platform.

**By tag** (any older release): `fly machine update "$M" -a <app> --image
registry.fly.io/<app>:<tag> -y` does take a tag. `fly releases -a
illogical-control --image` lists `fly deploy`'s tags (`vX.Y.Z`); a chant
release pushes `server-<run>`.

Releases deploy the platform first and the hub second. Roll back in the
reverse order, the hub first, unless only one of them is broken.

**An older build refuses a database it can't read.** If the release ran a
migration that isn't additive, the old image won't start (`fly logs` says
which migration). Then roll forward with a fix instead, or restore
("Restore", below).

**The release way:** `just server-release <sha>` releases an older green
commit through the release workflow (control-ops.md, "Deploys"). It takes a
build, so it's slower, but chant's ledger then agrees with what runs. After
an image rollback, the next release goes forward as usual.

## Restore

**Both databases can be restored** (checked 2026-11-11):
- **The hub's SQLite:** Litestream is on, replicating to R2 within about a
  second. `scripts/hub-restore-drill` restored production's backup from R2
  onto geek in 34 s: integrity `ok`, 63 tables, current to the second (and to
  an hour before with `--at`). Fly also keeps daily volume snapshots for 5
  days.
- **The platform's Postgres:** Fly's own backups (hourly incremental, daily
  full) and the nightly encrypted dump in R2 ("The platform's nightly dump",
  below). An hourly check on the operator's machine alerts when the newest
  dump is over 26 h old.

**Before the watch, and each day of it:** run both drills' quick checks,
`scripts/hub-restore-drill` and `scripts/platform-restore-drill --age`, on
geek with Infisical logged in, and check `arugula-dump-age-check.timer` is
listed.

### The platform's nightly dump

The `platform-dump` Machine in `arugula-platform` dumps the platform's
Postgres to R2, encrypted, about once a day: Fly starts it every hour, and
it dumps only when the newest dump is 20 h old (control-ops.md, "The
nightly dump"). **Fly's schedule never starts a Machine that hasn't been
started since its last update** (Arugula #867: no dump ran on its own for
18 days). Production's release starts it once after updating it, once
that fix has shipped; until then, check it's armed after each release
("Is it scheduled?", below). If you
`fly machine update` it by hand, start it once after, or it never runs
again.

**The alert:** `scripts/dump-age-check`, hourly on a systemd user timer on
the operator's machine, reads the newest dump's record in R2. When it's over
26 h old, or the check can't read it, it opens a native issue labelled
`backup`, assigned to the operator, or comments on the open one once a day.

**The last dump's age:**

```
scripts/dump-age-check --dry-run               # newest platform dump …, N h old (limit 26 h)
scripts/platform-restore-drill --age           # the same, exit 1 past 26 h
journalctl --user -u arugula-dump-age-check -n 20
```

Both read R2's keys from Infisical at each run, so run them on geek with
Infisical logged in.

**Is it scheduled?** A Machine that's armed shows a `start` from `flyd`
about every hour in its events; one that isn't shows the release's
`launch` and `update`, then nothing:

```
D=$(fly machine list -a arugula-platform --json | jq -r '.[] | select(.config.metadata.role == "platform-dump") | .id')
fly machine status "$D" -a arugula-platform                     # Event Logs
fly logs -a arugula-platform --machine "$D" --no-tail | grep platform-dump
```

**Run one by hand** (also what arms the schedule again). It skips while the
newest dump is under 20 h old: `platform-dump: skipped: …`. To force a dump,
start it with the gate at 0, then put it back. Each update leaves it
stopped, so start it after each one:

```
fly machine start "$D" -a arugula-platform
fly logs -a arugula-platform --machine "$D" --no-tail | grep platform-dump   # platform-dump: ok …

# A dump now, whatever the newest one's age:
fly machine update "$D" -a arugula-platform --env ARUGULA_DUMP_EVERY_HOURS=0 -y
fly machine start "$D" -a arugula-platform                      # dumps
fly machine update "$D" -a arugula-platform --env ARUGULA_DUMP_EVERY_HOURS=20 -y
fly machine start "$D" -a arugula-platform                      # skips, and arms the schedule
```

The next release puts back its declared config (no
`ARUGULA_DUMP_EVERY_HOURS`, so 20).

**Install the alert** (once, on the operator's machine):

```
INFISICAL_PROJECT_ID=<arugula's> scripts/dump-age-check --install   # copies it to ~/.local/share/arugula-dump-age, hourly
systemctl --user list-timers arugula-dump-age-check.timer
```

Its settings are in `~/.config/arugula-dump-age/env`: set
`ARUGULA_DUMP_AGE_PROJECT` to the project the `backup` issue goes in. To see
an alert end to end once, run it with `ARUGULA_DUMP_AGE_LIMIT_HOURS=0`, then
close the issue it opens.

### The hub

**From R2, onto a fresh volume** (the volume is lost or the database is
corrupt). Control restores the latest backup by itself when it starts with
no database. A Machine's volume can't be swapped, so the restored hub is a
clone of the old Machine on a new, empty volume. Stop the old one first:
two hubs replicating to one backup would ruin it.

```
M=$(fly machine list -a illogical-control --json | jq -r '.[] | select(.config.metadata.fly_process_group == "app") | .id')
fly machine stop "$M" -a illogical-control
fly machine clone "$M" -a illogical-control --region ewr     # a new, empty control_data volume
fly logs -a illogical-control --no-tail | grep -E 'restoring the latest backup|database restored'
fly status -a illogical-control                              # the clone started, 1/1 checks
```

Rehearsed end to end (2026-11-11), word for word, on a throwaway copy of the
hub running the production build and backing up to an R2 prefix of its own
(`LITESTREAM_PATH=rehearsal-…`, never `control`): stopped, cloned, and the
clone said `no database: restoring the latest backup` then `database
restored from its backup`, answering in 12 s with the same database (its
push key, which lives there, unchanged). The copy and its prefix were
deleted after. Production's own backup restores too: `scripts/hub-restore-drill`
above.

Check that the newest account or machine is there before declaring it
over, then `fly machine destroy "$M" -a illogical-control`. Keep the old
volume until the restore is proven, then `fly volumes destroy <old id>`.
Before the next release, put the new Machine's name in
`ops/src/fly/control/control.fly.ts` (the release finds the Machine by name
and the volume by its name, `control_data`, so only one may be left).

**From a Fly volume snapshot** (R2 unreachable). The same, with the clone's
volume made from the snapshot, which is up to a day old. Rehearsed on a copy
of the hub (2026-11-10): back in 10 s with the same database (its push key,
which lives in the database, unchanged). The clone gets a new name and a new
`control_data` volume:

```
fly volumes snapshots list vol_r68lmz2doox9myd4 -a illogical-control
fly machine stop "$M" -a illogical-control
fly machine clone "$M" -a illogical-control --region ewr --from-snapshot <vs_…>
```

**To a point in time** (a bad write, not a lost disk): Litestream restores
any moment of the last week into a file beside the live database, to read
from:

```
fly ssh console -a illogical-control -C "/litestream restore -config /data/litestream.yml -timestamp 2026-12-01T14:00:00Z -o /data/control-at-1400.db /data/control.db"
```

Putting the whole hub back to that moment isn't one command today: the
image has no shell to swap the files with, and a restore on start takes
only the latest. Recover the rows you need from the file by hand, or
restore it on another machine with the bucket's keys
([control-ops.md, "Restoring"](control-ops.md#restoring)) and bring the
file onto a fresh volume. `-dry-run` shows any restore's plan first.
Arugula #853 is the follow-up that would make it one step.

### The platform

Turn maintenance on and pause the platform first (control-ops.md, "The
platform"). Then, first choice, Fly's backups into a new cluster:

```
fly mpg backup list 1zqyxr76dp1owp8m
fly mpg restore 1zqyxr76dp1owp8m --backup-id <id> --name arugula-platform-db-restored    # or --pitr-time
```

A restore takes a few minutes to make the cluster (rehearsed on the staging
copy, 2026-11-10: 6 minutes to `ready`, every table and migration there).
Check it before pointing anything at it:

```
echo "select count(*) from information_schema.tables where table_schema='public'; select max(version) from platform_migrations;" |
  fly mpg connect <new cluster id> -d fly-db
```

Put the new cluster's connection string (Fly's dashboard, *Managed Postgres →
Connect*) in Infisical and in `ARUGULA_PLATFORM_DATABASE_URL`
(`fly secrets set -a arugula-platform ARUGULA_PLATFORM_DATABASE_URL=…`, which
restarts the machines one at a time), unpause, and turn maintenance off. Without Fly's backups, use the nightly dump
([control-ops.md, "Restoring the platform's Postgres"](control-ops.md#restoring-the-platforms-postgres)).

## Switches

Each one is flipped without a deploy.

- **Maintenance** (chat, Cloud and billing answer 503; terminals and the relay
  stay up):

  ```
  fly ssh console -a illogical-control -C "/arugula-control maintenance on --retry-after 30 --db /data/control.db"
  fly ssh console -a illogical-control -C "/arugula-control maintenance off --db /data/control.db"
  ```

- **The launch switches** (Arugula #531): pause sign-ups, sign-ups per
  address, new hosted boxes open, waitlisted or off, admissions, per-account
  caps and the kill switch. Each takes effect within a second, without a
  restart. What each does and what people see is in control-ops.md, "Launch
  switches". These are the commands as rehearsed on the staging copy
  (2026-11-11). Run each as a whole `-C "..."` string: a word with spaces
  (`--why`, `--message`) needs its own quotes inside it, and a helper that
  passes `$*` loses them.

  ```
  c() { fly ssh console -q -a illogical-control -C "/arugula-control switches $1 --db /data/control.db" | tr -d '\r'; }
  c "status"                                           # every switch; add an account for its own
  c "signups pause --message 'Back in an hour.'"       # or no --message, for the usual sentence
  c "signups open"
  c "cap signups-per-hour 30"                          # new accounts per address an hour (60)
  c "hosted off"                                       # or open, or waitlist (the default)
  c "waitlist"                                         # who asked for a box, first first
  c "admit github:LOGIN --by NAME"                     # an account id works too; several at once
  c "unadmit ACCOUNT"
  c "cap boxes 5 --account github:LOGIN"               # boxes, box-hours, relay-mb; 'default' puts one back
  c "suspend github:LOGIN --by NAME --why 'mining on a hosted box'"
  c "unsuspend ACCOUNT"
  ```

  The caps decided for the beta: per account, 2 hosted boxes, 60 box-hours a
  month and 10 GB of relay a day; sign-ups at most 60 an hour per address;
  hosted boxes waitlist-only. (TURN minutes aren't capped: control hands out
  no TURN credentials.) Note them in the shift's handoff when you change one.
  What the rehearsal showed:
  - A suspended account's page, platform calls and machines get 403 at once,
    and its relay sockets drop; nobody else's notice.
  - Past `relay-mb`, relayed traffic slows to about 64 KB/s per account: a busy
    pane's keystroke echo went from about 20 ms to over 10 s. Raise an
    account's cap rather than leave someone working there.
  - The staging copy's generator addresses (`ARUGULA_LOADTEST_EXEMPT_IPS`)
    skip `signups-per-hour`; production has no such exemption.
- **Who may use hosted workspaces** today: `ARUGULA_HOSTED_ACCOUNTS` on
  `arugula-platform` (a secret: setting it restarts the platform's machines
  one at a time).

## Spend

Fly has no billing alerts, so a daily check does it: `scripts/spend-check`,
on a systemd user timer on the operator's machine.

It reckons the month from Fly's metrics and API:
- each machine's hours at its size;
- egress;
- volumes, IPv4s and Postgres clusters;
- E2B, from the platform's own reckoning.

It prints the month so far, the last 24 hours and a projection for the month.
When the projection passes `ARUGULA_SPEND_MONTH_LIMIT` ($400) or the last 24
hours pass `ARUGULA_SPEND_DAY_LIMIT` ($30), it opens a native issue labelled
`spend`, assigned to the operator, or comments on the open one once a day.
It covers every app in the Fly org, so a staging or test app left running
shows up by name. It's an estimate: Fly's invoice (*Billing* in Fly's
dashboard) is the truth, and the check leaves out snapshots past 10 GB,
stopped machines' disks, and credits.

```
scripts/spend-check --dry-run           # the report now; writes nothing
scripts/spend-check --install           # once: copies it to ~/.local/share/arugula-spend, daily at 13:00 UTC
systemctl --user list-timers arugula-spend-check.timer
journalctl --user -u arugula-spend-check -n 40
```

Its limits are in `~/.config/arugula-spend/env`. To see an alert end to end
once, run it with `ARUGULA_SPEND_DAY_LIMIT=0`, then close the issue it opens.
It runs as the operator's `arugula` and `fly` logins. An agent's run can't
open the issue: agents write native issues through MCP, as themselves.

**When it fires:** find the line that grew in its table. A machine nobody
needs gets `fly machine stop` or `fly apps destroy` (staging and test apps).
Hosted boxes: the #531 caps and the hosted switch. Egress: relay bytes on the
dashboard, then the per-account relay cap.

**Caps on everything metered.** Fly has none, hence the check. The rest
have caps in their own consoles. Set each before the launch:

| Service | What's metered | Where its cap is |
|---|---|---|
| Fly.io | machines, egress, Postgres | none: the daily check above |
| E2B | box seconds | the team's usage limit in E2B's dashboard, plus #531's per-account box hours |
| Anthropic (hosted agents' inference) | tokens | the workspace's spend limit in the Anthropic console |
| PostHog | events past the free tier | *Billing → billing limit* (control drops events when its queue is full; a cap only stops PostHog ingesting) |
| Resend | emails past the free tier | the plan's own limit |
| Cloudflare R2 | storage and operations | free tier; nothing to set at this size |

## Status page

**https://status.arugula.io**, linked from the site's footer, the docs and the
README. It's [arugula-salad/status](https://github.com/arugula-salad/status):
UptimeFlare on Cloudflare Workers and Pages, checking every minute from
Cloudflare's network. It runs off Fly, so it says "down" when control is.
It watches control (`/control.json`), the platform (through
`app.arugula.io`), `install.sh`, the newest release's downloads, the site and
the docs.

**Post an incident** as soon as the dashboard shows something people will
notice: add an entry to `maintenances` in that repository's `uptime.config.ts`
and push to `main` (the deploy takes about three minutes; its README has the
exact shape):

```ts
{ monitors: ['control'], title: 'Investigating slow sign-ins',
  body: "Some people can't sign in. We're looking into it.",
  start: '2026-12-01T14:05:00Z', color: 'red' },
```

Update `body` as it goes, and add `end` when it's over. Watch the deploy with
`gh run list -R arugula-salad/status -L 1`.

## After the 48 hours

- Scale back down what was scaled up, or land the PRs that keep it.
- Destroy staging and test apps, and clones made for the peak.
- Write up what broke and what the dashboard missed, in the launch thread.
