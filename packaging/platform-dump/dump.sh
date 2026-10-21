#!/usr/bin/env bash
#
# The platform's nightly dump (M145, docs/control-ops.md "The nightly
# dump"): pg_dump of the platform's Postgres, encrypted to age public keys,
# put in R2 with a record of what it holds. One run, then exit: a Fly
# machine with `--schedule daily` runs it.
#
#   1. the migration check: the database's migrations are this build's
#      (`arugula-platform migrations`, read-only)
#   2. one snapshot, exported by a session held open, that both the
#      checksums and pg_dump read, so the record is exactly the dump
#   3. pg_dump --format=custom | age -r … → dump.age (no key here opens it)
#   4. put DUMP.pgdump.age, then DUMP.json (the record), then latest.json
#   5. read the dump's size back from R2
#
# Each run logs `platform-dump: ok …` last, or `platform-dump: FAILED …`
# and exits non-zero (fly logs; the machine's exit code in `fly machine
# status`). With ARUGULA_DUMP_PING_URL it also pings a dead man's switch
# (healthchecks.io's convention: URL/start, URL, URL/fail), which notices a
# run that never happened.
#
# Environment (names only here; values are Fly secrets or the machine's env):
#   ARUGULA_PLATFORM_DATABASE_URL  the cluster's direct URL (the app's secret), or
#   ARUGULA_DUMP_DATABASE_URL      one for a user of its own (`fly mpg users create
#                                  … -r reader`), when it's set
#   ARUGULA_DUMP_RECIPIENTS        age public keys (age1…), space- or comma-separated
#   ARUGULA_DUMP_BUCKET, ARUGULA_DUMP_ENDPOINT, ARUGULA_DUMP_ACCESS_KEY_ID,
#   ARUGULA_DUMP_SECRET_ACCESS_KEY R2, else the app's LITESTREAM_* (the same bucket)
#   ARUGULA_DUMP_PREFIX            the objects' prefix (platform-pg), which R2's
#                                  lifecycle rule expires after 30 days
#   ARUGULA_DUMP_REGION            SigV4's region (auto, R2's)
#   ARUGULA_DUMP_PLATFORM_URL      the platform's health check, for its version
#                                  (http://arugula-platform.internal:7700/platform.json)
#   ARUGULA_DUMP_PING_URL          optional: the dead man's switch
set -euo pipefail
umask 077

log() { echo "platform-dump: $*"; }
step="starting"
work=""
ping() {
  [ -n "${ARUGULA_DUMP_PING_URL:-}" ] || return 0
  curl -fsS -m 10 --retry 3 -o /dev/null --data-raw "${2:-}" "${ARUGULA_DUMP_PING_URL%/}$1" ||
    log "warning: couldn't ping the dead man's switch ($1)"
}
finish() {
  local rc=$?
  trap - EXIT
  [ -z "$work" ] || rm -rf "$work"
  if [ "$rc" != 0 ]; then
    log "FAILED at $step (exit $rc)"
    ping /fail "platform-dump failed at $step"
  fi
  exit "$rc"
}
trap finish EXIT

url=${ARUGULA_DUMP_DATABASE_URL:-${ARUGULA_PLATFORM_DATABASE_URL:?is unset}}
bucket=${ARUGULA_DUMP_BUCKET:-${LITESTREAM_BUCKET:?no ARUGULA_DUMP_BUCKET or LITESTREAM_BUCKET}}
endpoint=${ARUGULA_DUMP_ENDPOINT:-${LITESTREAM_ENDPOINT:?no ARUGULA_DUMP_ENDPOINT or LITESTREAM_ENDPOINT}}
key_id=${ARUGULA_DUMP_ACCESS_KEY_ID:-${LITESTREAM_ACCESS_KEY_ID:?no R2 access key id}}
secret=${ARUGULA_DUMP_SECRET_ACCESS_KEY:-${LITESTREAM_SECRET_ACCESS_KEY:?no R2 secret access key}}
prefix=${ARUGULA_DUMP_PREFIX:-platform-pg}
region=${ARUGULA_DUMP_REGION:-auto}
health=${ARUGULA_DUMP_PLATFORM_URL:-http://arugula-platform.internal:7700/platform.json}
step="checking the recipients"
read -r -a recipients <<<"$(tr ',' ' ' <<<"${ARUGULA_DUMP_RECIPIENTS:-}")"
[ "${#recipients[@]}" -gt 0 ] || { log "no ARUGULA_DUMP_RECIPIENTS (age public keys)"; exit 2; }
age_args=()
for r in "${recipients[@]}"; do
  case "$r" in age1*) age_args+=(-r "$r") ;; *) log "not an age public key: $r"; exit 2 ;; esac
done

work=$(mktemp -d)
ping /start

# The platform's version beside this build's: a dump image a release
# behind still dumps, and the record says so.
step="reading the platform's version"
mine=$(/arugula-platform --version | awk '{ print $NF }')
running=$(curl -fsS -m 5 "$health" 2>/dev/null | jq -r '.version // empty' 2>/dev/null || true)
if [ -z "$running" ]; then
  log "warning: $health didn't say the platform's version"
elif [ "$running" != "$mine" ]; then
  log "warning: this image carries arugula-platform $mine, the platform runs $running"
fi

step="checking the migrations"
/arugula-platform migrations --db "$url" > "$work/migrations.txt"

# A session holding a snapshot open: pg_dump and the checksums both read
# it, so the record describes exactly the rows in the dump, whatever is
# written meanwhile.
step="exporting a snapshot"
coproc HOLD { psql -X -q -t -A -v ON_ERROR_STOP=1 "$url" 2>&1; }
# shellcheck disable=SC2153 # coproc sets HOLD_PID
hold_pid=$HOLD_PID
echo "BEGIN ISOLATION LEVEL REPEATABLE READ, READ ONLY; SELECT pg_export_snapshot();" >&"${HOLD[1]}"
snapshot=""
read -r -t 30 snapshot <&"${HOLD[0]}" || true
[[ $snapshot =~ ^[0-9A-Fa-f-]+$ ]] || { log "psql didn't export a snapshot: $snapshot"; exit 1; }

step="reading the checksums"
/arugula-platform checksums --db "$url" --snapshot "$snapshot" > "$work/checksums.txt"

step="dumping"
at=$(date -u +%Y-%m-%dT%H%M%SZ)
started=$(date +%s)
pg_dump --snapshot="$snapshot" --format=custom --no-owner --no-privileges --schema=public "$url" |
  age "${age_args[@]}" -o "$work/dump.age"
server=$(psql -X -q -t -A "$url" -c "SHOW server_version")

step="letting the snapshot go"
echo "COMMIT;" >&"${HOLD[1]}"
eval "exec ${HOLD[1]}>&-"
wait "$hold_pid" || true

step="writing the record"
object="$prefix/$at.pgdump.age"
bytes=$(stat -c %s "$work/dump.age")
sha=$(sha256sum "$work/dump.age" | cut -d' ' -f1)
jq -n --arg at "$at" --arg object "$object" --argjson bytes "$bytes" --arg sha256 "$sha" \
  --arg platform "$mine" --arg running "$running" --arg pg_dump "$(pg_dump --version)" --arg server "$server" \
  --arg recipients "${recipients[*]}" --argjson seconds "$(($(date +%s) - started))" \
  --rawfile migrations "$work/migrations.txt" --rawfile sums "$work/checksums.txt" '{
    at: $at, object: $object, bytes: $bytes, sha256: $sha256, seconds: $seconds,
    platform: $platform, platform_running: $running, pg_dump: $pg_dump, server: $server,
    recipients: ($recipients | split(" ")),
    migrations: [$migrations | split("\n")[]
      | capture("^ *(?<version>[0-9]+)  (?<name>\\S+) +(?<additive>yes|no) +(?<checksum>[0-9a-f]+)$")
      | .version |= tonumber],
    checksums: [$sums | split("\n")[]
      | capture("^(?<table>\\S+) +(?<rows>[0-9]+)  (?<sha256>[0-9a-f]{64})$")
      | .rows |= tonumber]
  }' > "$work/record.json"
tables=$(jq '.checksums | length' "$work/record.json")
rows=$(jq '[.checksums[].rows] | add // 0' "$work/record.json")
[ "$tables" -gt 0 ] || { log "the checksums listed no table"; exit 1; }

# PUT with SigV4; the credentials go to curl on stdin, not its arguments.
s3() {
  printf 'user = "%s:%s"\n' "$key_id" "$secret" |
    curl -fsS -K - --retry 3 -m 600 --aws-sigv4 "aws:amz:$region:s3" "$@"
}
put() {
  s3 -o /dev/null -H "x-amz-content-sha256: $(sha256sum "$2" | cut -d' ' -f1)" -H "content-type: $3" \
    --upload-file "$2" "${endpoint%/}/$bucket/$1"
}
step="putting $object"
put "$object" "$work/dump.age" application/octet-stream
step="putting the record"
put "${object%.pgdump.age}.json" "$work/record.json" application/json
put "$prefix/latest.json" "$work/record.json" application/json

step="reading $object back"
have=$(s3 -I "${endpoint%/}/$bucket/$object" | tr -d '\r' | awk 'tolower($1) == "content-length:" { print $2 }')
[ "$have" = "$bytes" ] || { log "R2 holds $have bytes of $object, not $bytes"; exit 1; }

log "ok $object: $bytes bytes, sha256 ${sha:0:16}, $tables tables, $rows rows, migrations up to $(jq '.migrations[-1].version' "$work/record.json"), $(jq .seconds "$work/record.json") s"
ping "" "$object $bytes bytes $rows rows"
