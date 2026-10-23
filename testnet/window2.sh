#!/usr/bin/env bash
#
# M145's window, rehearsed on the testnet (docs/control-ops.md, "The second
# window (M145)"): the runbook's steps in order, from the platform out of
# process on its SQLite file (production since M141) to two machines on
# Postgres, with the nightly dump, the restore drill and the rollbacks.
#
#   testnet/window2.sh       bring the stack up, rehearse, leave it up
#   just testnet down        afterwards
#
# What it does, as production will:
#   1. up: control as the gateway, the platform out of process on its file,
#      Postgres beside it, an S3 store for R2
#   2. a person's chat and a project's secret, written through the gateway
#      (window.ts seed)
#   3. the notice, a day ahead
#   4. pre-checks: a timed trial copy into Postgres, its migrations, and its
#      secrets open with the platform's key
#   5. the window: maintenance on, the platform paused, portable checksums
#      of the file, `copy --replace` into Postgres, Postgres's and the
#      file's checksums both equal to them, secrets open; the flip onto
#      Postgres (paused), rolled back once to the untouched file and done
#      again; unpaused; maintenance off
#   6. post-checks, then new work written on Postgres
#   7. two machines: platform2 on Postgres (`fly scale count 2`), exactly one
#      leads; the hub switched to `platforms` (Flycast's stand-in); both
#      serve: through the gateway, the platform-fanout claim, and each
#      machine stopped in turn while the gateway keeps answering
#   8. the first nightly dump by hand, the restore drill against it (and a
#      drill against a record that lies, which must fail), a dump that
#      can't reach the database, which must fail loudly
#   9. rollback after people wrote on Postgres: one machine, the hub back on
#      `.internal`, Postgres copied back over the file, the platform back
#      on its file, with that new work there
#
# A transcript is kept in $STATE/window/, and the summary is printed at the
# end. Docker stands in for Fly: a recreated container for a secret and its
# restart, `docker compose up platform2` for `fly scale count 2`, `platforms`
# for Flycast.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
# shellcheck source-path=SCRIPTDIR source=env.sh
. "$HERE/env.sh"
need_docker

W="$STATE/window"
PG=postgres://postgres:testnet@postgres:5432/platform
export ARUGULA_TESTNET_UID ARUGULA_TESTNET_GID
ARUGULA_TESTNET_UID="$(id -u)"
ARUGULA_TESTNET_GID="$(id -g)"
# Where the hub sends platform calls, the platform's database (empty: its
# file), and whether it's held: what Fly's secrets say in production.
export ARUGULA_TESTNET_PLATFORM_URL=http://platform:7700 ARUGULA_TESTNET_PLATFORM_DATABASE_URL="" ARUGULA_TESTNET_PLATFORM_PAUSED=0
export ARUGULA_TESTNET_PLATFORM_PORT="${ARUGULA_TESTNET_PLATFORM_PORT:-22983}"
export ARUGULA_TESTNET_PLATFORM2_PORT="${ARUGULA_TESTNET_PLATFORM2_PORT:-22984}"
export ARUGULA_TESTNET_S3_PORT="${ARUGULA_TESTNET_S3_PORT:-22986}"
export ARUGULA_TESTNET_DUMP_RECIPIENTS=""

log() { echo "[testnet window2] $*" >&2; }
die() { log "FAIL: $*"; exit 1; }
step() { echo; echo "== $*"; echo "== $*" >> "$W/transcript.txt"; }
now() { date +%s.%N; }
since() { awk -v a="$1" -v b="$(now)" 'BEGIN { printf "%.1f", b - a }'; }
between() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.1f", b - a }'; }
dc() { docker compose -f "$HERE/compose.yaml" -f "$HERE/window.yaml" -f "$HERE/window2.yaml" --profile ssh --profile control --profile platform --profile two --profile dump "$@"; }
hub() { docker exec "$TESTNET-control" /opt/arugula/arugula-control "$@"; }
plat() { docker exec "$TESTNET-platform" /opt/arugula/arugula-platform "$@"; }
win() { node --experimental-strip-types --no-warnings "$HERE/window.ts" --state "$W/device.json" "$@"; }
claim() { "$HERE/test.sh" control "$@"; }
record() { tee -a "$W/transcript.txt"; }
# A checksums' output, by table: rows and the whole sha256.
sums() { awk 'NF == 3 && length($3) == 64 { print $1, $2, $3 }' "$1" | sort; }
# A copy's "to" checksums (16 hex characters), and checksums' cut to match.
copied() { awk 'NF == 6 && $NF == "ok" { print $1, $3, $5 }' "$1" | sort; }
cut16() { awk '{ print $1, $2, substr($3, 1, 16) }'; }
logged() { docker logs "$1" > "$W/log.txt" 2>&1; grep -q "$2" "$W/log.txt"; }
answers() { curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$cport$1"; }
must_503() {
  local code
  code="$(answers /api/chat)"
  [ "$code" = 503 ] || die "GET /api/chat answered $code during the window, not 503"
}
# A machine's /platform.json once it says its database answers.
healthy() {
  local body=""
  for _ in $(seq 1 60); do
    body="$(curl -fsS "http://127.0.0.1:$1/platform.json" 2>/dev/null || true)"
    case "$body" in *'"database":true'*) echo "$body"; return 0 ;; esac
    sleep 0.5
  done
  die "the platform on :$1 isn't healthy: $body"
}

control_up() {
  dc up -d --wait control >/dev/null 2>&1 || dc up -d control
  for _ in $(seq 1 60); do
    curl -fsS "http://127.0.0.1:$cport/control.json" >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  docker logs --tail 30 "$TESTNET-control" >&2
  die "control didn't answer"
}
# A platform machine (re)started as the variables say: a Fly secret and the
# restart it makes.
machine_up() {
  local svc=$1 port=$2
  dc up -d --wait "$svc" >/dev/null 2>&1 || dc up -d "$svc"
  healthy "$port" >/dev/null
}
platform_up() { machine_up platform "$ARUGULA_TESTNET_PLATFORM_PORT"; }
platform2_up() { machine_up platform2 "$ARUGULA_TESTNET_PLATFORM2_PORT"; }
# The URL into the platform's /tmp, as `fly ssh sftp put` puts it there: the
# tools read it as @/tmp/pg.url, so no password is on a command line. A
# restart clears it, as on Fly.
put_url() {
  (umask 077 && printf '%s\n' "$PG" > "$W/pg.url")
  docker cp -a -q "$W/pg.url" "$TESTNET-platform:/tmp/pg.url"
  rm -f "$W/pg.url"
}

# ---- 1. up
rm -rf "$W"
mkdir -p "$W/hub" "$W/platform" "$W/platform2" "$W/s3/backups"
: > "$W/transcript.txt"
step "1. up: control as the gateway, the platform out of process on its file"
ARUGULA_TESTNET_PLATFORM=in "$HERE/up.sh" control
# shellcheck source=/dev/null
. "$STATE/control.env"
cport="$ARUGULA_TESTNET_CONTROL_PORT"
# The dump's image, around this tree's static binary, as
# scripts/ci-platform-deploy builds it; and its age keys (the testnet's own).
ctx="$(mktemp -d)"
cp "$ARUGULA_TESTNET_BINARIES/arugula-platform" "$ROOT/packaging/platform-dump/Dockerfile" "$ROOT/packaging/platform-dump/dump.sh" "$ctx/"
docker build -q -t "$TESTNET-platform-dump" "$ctx" >/dev/null
rm -rf "$ctx"
age-keygen -o "$W/dump.key" 2>/dev/null
ARUGULA_TESTNET_DUMP_RECIPIENTS="$(age-keygen -y "$W/dump.key")"
dc up -d --wait fakes s3 >/dev/null 2>&1 || dc up -d fakes s3
platform_up
control_up
logged "$TESTNET-control" 'the platform runs out of process' || die "control isn't the gateway"
echo "control: the gateway; the platform: out of process on /window/platform.db" | record

# ---- 2. seed
step "2. a person's chat and secrets, before the window"
# shellcheck disable=SC2086 # CONTROL_VIA is several words
node --experimental-strip-types --no-warnings "$ROOT/web/fixtures/device-cli.ts" --state "$W/device.json" \
  signin --control "$CONTROL_URL" $CONTROL_VIA --login "window2-$$" > /dev/null
win seed > "$W/seed.json"
record < "$W/seed.json"
win check "$W/seed.json" | record

# ---- 3. the notice
step "3. the notice, a day ahead"
title="Chat and Cloud maintenance"
hub notice post --db /window/control.db --title "$title" --body "Chat and Cloud will be down for a few minutes." | record
win notice "$title" | record

# ---- 4. pre-checks
step "4. pre-checks: a timed trial copy into Postgres; its migrations and secrets"
plat migrations --db /window/platform.db | record
put_url
t="$(now)"
plat copy --from /window/platform.db --to @/tmp/pg.url > "$W/trial.txt"
echo "trial copy: $(tail -n 1 "$W/trial.txt") in $(since "$t") s" | record
plat migrations --db @/tmp/pg.url | record
plat secrets-check --db @/tmp/pg.url | record

# ---- 5. the window
step "5. the window"
t0="$(now)"
hub maintenance on --retry-after 30 --db /window/control.db | record
sleep 2
must_503
[ "$(answers /control.json)" = 200 ] || die "the hub's own routes went down"
echo "GET /api/chat: 503; GET /control.json: 200" | record
ARUGULA_TESTNET_PLATFORM_PAUSED=1
platform_up
logged "$TESTNET-platform" 'paused: no job worker' || die "the platform isn't held"
put_url
plat checksums --db /window/platform.db --portable > "$W/before.txt"
t="$(now)"
plat copy --from /window/platform.db --to @/tmp/pg.url --replace > "$W/copy.txt"
copy_s="$(since "$t")"
record < "$W/copy.txt"
diff <(sums "$W/before.txt" | cut16) <(copied "$W/copy.txt") || die "the copy's checksums aren't the file's"
plat checksums --db @/tmp/pg.url > "$W/pg.txt"
plat checksums --db /window/platform.db --portable > "$W/file-after.txt"
diff <(sums "$W/before.txt") <(sums "$W/pg.txt") || die "Postgres's checksums differ from before.txt"
diff <(sums "$W/before.txt") <(sums "$W/file-after.txt") || die "the file changed after before.txt"
echo "checksums: before.txt = Postgres = the file now ($(sums "$W/before.txt" | wc -l) tables)" | record
plat secrets-check --db @/tmp/pg.url | record
t1="$(now)"
# The flip, still paused: one restart onto Postgres.
ARUGULA_TESTNET_PLATFORM_DATABASE_URL="$PG"
platform_up
logged "$TESTNET-platform" 'this machine leads' || die "the platform on Postgres doesn't lead"
healthy "$ARUGULA_TESTNET_PLATFORM_PORT" | record
plat checksums --db env:ARUGULA_PLATFORM_DATABASE_URL > "$W/pg-flipped.txt"
diff <(sums "$W/before.txt") <(sums "$W/pg-flipped.txt") || die "Postgres changed across the flip"
echo "on Postgres (the server's URL, env:ARUGULA_PLATFORM_DATABASE_URL): still before.txt's checksums" | record
# Rollback before anyone wrote: unset the URL, back on the untouched file.
step "5r. rollback before anyone wrote: the URL unset, the file as it was"
ARUGULA_TESTNET_PLATFORM_DATABASE_URL=""
platform_up
plat checksums --db /window/platform.db --portable > "$W/file-rollback.txt"
diff <(sums "$W/before.txt") <(sums "$W/file-rollback.txt") || die "the file changed"
echo "back on the file; its checksums still before.txt's" | record
ARUGULA_TESTNET_PLATFORM_DATABASE_URL="$PG"
platform_up
t2="$(now)"
ARUGULA_TESTNET_PLATFORM_PAUSED=0
platform_up
if logged "$TESTNET-platform" 'paused: no job worker'; then die "the platform is still held"; fi
t3="$(now)"
hub maintenance off --db /window/control.db | record
sleep 2
echo "window: $(since "$t0") s of 503s (copy ${copy_s} s; copy and checks $(between "$t0" "$t1") s; flip, its rollback and the flip again $(between "$t1" "$t2") s; unpause $(between "$t2" "$t3") s)" | record

# ---- 6. post-checks
step "6. post-checks on Postgres, then new work there"
win check "$W/seed.json" | record
claim platform | record
win write pg "$W/seed.json" | record
put_url
plat secrets-check --db @/tmp/pg.url > "$W/secrets-pg.txt"
record < "$W/secrets-pg.txt"
grep -q '^  1 open$' "$W/secrets-pg.txt" || die "the secret set on Postgres doesn't open with the platform's key"
echo "the secret set again on Postgres, sealed with the project's key: it opens" | record

# ---- 7. two machines
step "7. two machines, and the hub through Flycast's stand-in"
platform2_up
h1="$(healthy "$ARUGULA_TESTNET_PLATFORM_PORT")"
h2="$(healthy "$ARUGULA_TESTNET_PLATFORM2_PORT")"
echo "platform:  $h1" | record
echo "platform2: $h2" | record
leaders=$(printf '%s\n%s\n' "$h1" "$h2" | grep -c '"leader":true' || true)
[ "$leaders" = 1 ] || die "$leaders machines lead"
ARUGULA_TESTNET_PLATFORM_URL=http://platforms:7700
control_up
echo "the hub calls http://platforms:7700" | record
win gateway 6 "$W/seed.json" | record
# The claims test.sh runs on two machines (up.sh wrote `in` when it started).
sed -i 's/^export ARUGULA_TESTNET_PLATFORM=.*/export ARUGULA_TESTNET_PLATFORM=two/' "$STATE/control.env"
claim platform-fanout | record
for m in platform platform2; do
  dc stop "$m" >/dev/null 2>&1
  sleep 2
  echo "$m stopped: $(win gateway 3 "$W/seed.json")" | record
  if [ "$m" = platform ]; then platform_up; else platform2_up; fi
done
leaders=$( (healthy "$ARUGULA_TESTNET_PLATFORM_PORT"; healthy "$ARUGULA_TESTNET_PLATFORM2_PORT") | grep -c '"leader":true' || true)
[ "$leaders" = 1 ] || die "$leaders machines lead after the stops"
echo "each machine served through the gateway alone; one leads" | record

# ---- 8. the first nightly dump, by hand, and the drill
step "8. the first nightly dump by hand, and the restore drill against it"
dc run --rm dump 2>&1 | record
drill() {
  ARUGULA_DRILL_ENDPOINT="http://127.0.0.1:$ARUGULA_TESTNET_S3_PORT" ARUGULA_DRILL_BUCKET=backups \
    ARUGULA_DRILL_ACCESS_KEY_ID=testnet-key ARUGULA_DRILL_SECRET_ACCESS_KEY=testnet-secret \
    "$ROOT/scripts/platform-restore-drill" --identity "$W/dump.key" --bin "$ARUGULA_TESTNET_BINARIES/arugula-platform" "$@"
}
# The testnet's master key (window.yaml's), as Infisical holds production's.
ARUGULA_CLOUD_SECRET_KEY=c10dc10dc10dc10dc10dc10dc10dc10dc10dc10dc10dc10dc10dc10dc10dc10d drill --secrets 2>&1 | record
drill --age 2>&1 | record
# A record that lies about one table: the drill must say so. Put through
# the S3 store, as the dump job puts them.
s3() {
  AWS_ACCESS_KEY_ID=testnet-key AWS_SECRET_ACCESS_KEY=testnet-secret AWS_REGION=auto \
    aws --endpoint-url "http://127.0.0.1:$ARUGULA_TESTNET_S3_PORT" s3 "$@"
}
s3 cp --only-show-errors s3://backups/platform-pg/latest.json "$W/latest.json"
s3 cp --only-show-errors "s3://backups/$(jq -r .object "$W/latest.json")" s3://backups/platform-pg/tampered.pgdump.age
jq '.object = "platform-pg/tampered.pgdump.age" | .checksums[1].rows += 1' "$W/latest.json" > "$W/tampered.json"
s3 cp --only-show-errors "$W/tampered.json" s3://backups/platform-pg/tampered.json
if drill --object platform-pg/tampered.pgdump.age > "$W/drill-tampered.txt" 2>&1; then
  die "the drill passed a record that lies"
fi
grep -q DIFFERS "$W/drill-tampered.txt" || die "the drill failed without saying which table: $(tail -n 3 "$W/drill-tampered.txt")"
echo "a record that lies: the drill fails: $(grep DIFFERS "$W/drill-tampered.txt")" | record
s3 rm --only-show-errors s3://backups/platform-pg/tampered.pgdump.age
s3 rm --only-show-errors s3://backups/platform-pg/tampered.json
# Retention: the job prunes dumps older than 30 days, never the newest 3.
# Four old pairs seeded beside this run's first dump; the next run keeps
# itself, the first dump and the 35-day one (the newest 3), and deletes the
# rest. Then a prune that fails (the listing refused, by a curl in front of
# the real one) still leaves the run a success, with its dump in.
declare -A old
echo "old" > "$W/old.bin"
for days in 35 40 45 60; do
  old[$days]="$(date -u -d "$days days ago" +%Y-%m-%dT%H%M%SZ)"
  s3 cp --only-show-errors "$W/old.bin" "s3://backups/platform-pg/${old[$days]}.pgdump.age"
  s3 cp --only-show-errors "$W/old.bin" "s3://backups/platform-pg/${old[$days]}.json"
done
dc run --rm dump 2>&1 | grep 'platform-dump: \(ok\|pruned\)' | record
s3 ls s3://backups/platform-pg/ | awk '{ print $4 }' > "$W/after-prune.txt"
for t in "${old[40]}" "${old[45]}" "${old[60]}"; do
  if grep -q "^$t\." "$W/after-prune.txt"; then die "the prune kept $t, older than 30 days and not among the newest 3"; fi
done
for ext in pgdump.age json; do
  grep -q "^${old[35]}.$ext$" "$W/after-prune.txt" || die "the prune deleted ${old[35]}.$ext, one of the newest 3"
done
[ "$(grep -c '\.pgdump\.age$' "$W/after-prune.txt")" = 3 ] || die "not 3 dumps after the prune: $(cat "$W/after-prune.txt")"
echo "retention: 40, 45 and 60 days old deleted; the 35-day one kept as one of the newest 3; 3 dumps left" | record
cat > "$W/curl" <<'SH'
#!/bin/sh
case "$*" in *list-type=2*) echo "curl: (22) the listing refused (rehearsal)" >&2; exit 22 ;; esac
exec /usr/bin/curl "$@"
SH
chmod +x "$W/curl"
dc run --rm -v "$W/curl:/usr/local/bin/curl:ro" dump > "$W/dump-prune-fails.txt" 2>&1 || die "a failed prune failed the run: $(tail -n 3 "$W/dump-prune-fails.txt")"
grep -q 'platform-dump: warning: pruning old dumps failed' "$W/dump-prune-fails.txt" || die "a failed prune wasn't logged"
s3 ls s3://backups/platform-pg/ | grep -c '\.pgdump\.age$' | grep -qx 4 || die "the run whose prune failed didn't put its dump"
echo "a failed prune: logged, exit 0, its dump in" | record
# A dump that can't reach its database fails loudly.
if dc run --rm -e ARUGULA_PLATFORM_DATABASE_URL=postgres://postgres:wrong@postgres:5432/platform dump > "$W/dump-broken.txt" 2>&1; then
  die "a dump with the wrong password succeeded"
fi
grep 'platform-dump: FAILED' "$W/dump-broken.txt" | record

# ---- 9. rollback after the window
step "9. rollback after people wrote on Postgres: back to the file"
hub maintenance on --retry-after 30 --db /window/control.db | record
ARUGULA_TESTNET_PLATFORM_PAUSED=1
platform_up
platform2_up
dc rm -sf platform2 >/dev/null 2>&1
ARUGULA_TESTNET_PLATFORM_URL=http://platform:7700
control_up
plat copy --from env:ARUGULA_PLATFORM_DATABASE_URL --to /window/back.db > "$W/back.txt"
plat copy --from env:ARUGULA_PLATFORM_DATABASE_URL --to /window/platform.db --replace > "$W/back-file.txt"
record < "$W/back-file.txt"
diff <(copied "$W/back.txt") <(copied "$W/back-file.txt") || die "the two copies back differ"
ARUGULA_TESTNET_PLATFORM_DATABASE_URL=""
platform_up
ARUGULA_TESTNET_PLATFORM_PAUSED=0
platform_up
hub maintenance off --db /window/control.db | record
sleep 2
win check --has "after the window: pg" "$W/seed.json" | record
claim platform | record

step "done"
grep -h '^window: \|^trial copy' "$W/transcript.txt" || true
log "transcript: $W/transcript.txt; the stack is still up: just testnet down"
