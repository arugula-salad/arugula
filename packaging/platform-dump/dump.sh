#!/usr/bin/env bash
#
# The platform's nightly dump (M145, docs/control-ops.md "The nightly
# dump"): pg_dump of the platform's Postgres, encrypted to age public keys,
# put in R2 with a record of what it holds. One run, then exit: a Fly
# machine with `--schedule hourly` starts it, and a run skips unless the
# newest dump is ARUGULA_DUMP_EVERY_HOURS (20) old, so it dumps about once a
# day. Hourly, not daily, because a machine update (each deploy moves this
# machine onto the release's image) restarts Fly's schedule clock: under
# `daily`, deploys less than 24 h apart kept pushing the run back, and from
# 2026-10-21 to 2026-10-23 it never ran.
#
#   0. the gate: read `$prefix/latest.json` from R2; while its dump is
#      younger than ARUGULA_DUMP_EVERY_HOURS, log `platform-dump: skipped:
#      the newest dump is Nh old` and exit 0. A latest.json that's missing
#      or can't be read or parsed means dump
#   1. the migration check: the database's migrations are this build's
#      (`arugula-platform migrations`, read-only)
#   2. one snapshot, exported by a session held open, that both the
#      checksums and pg_dump read, so the record is exactly the dump
#   3. pg_dump --format=custom | age -r … → dump.age (no key here opens it)
#   4. put DUMP.pgdump.age, then DUMP.json (the record), then latest.json
#   5. read the dump's size back from R2
#   6. prune: delete the dumps (and their records) older than 30 days, but
#      never the newest 3, whatever their age, so a stopped clock or a run
#      of failures can't empty the prefix. A failed prune only logs: the
#      dump is in
#
# Each run logs `platform-dump: ok …`, `platform-dump: skipped: …`, or
# `platform-dump: FAILED …` and exits non-zero (fly logs; the machine's exit
# code in `fly machine status`). With ARUGULA_DUMP_PING_URL it also pings a
# dead man's switch (healthchecks.io's convention: URL/start, URL, URL/fail),
# which notices a run that never happened. A skip pings URL too: a dump
# younger than ARUGULA_DUMP_EVERY_HOURS is in R2.
#
# Environment (names only here; values are Fly secrets or the machine's env):
#   ARUGULA_PLATFORM_DATABASE_URL  the cluster's direct URL (the app's secret), or
#   ARUGULA_DUMP_DATABASE_URL      one for a user of its own (`fly mpg users create
#                                  … -r reader`), when it's set
#   ARUGULA_DUMP_RECIPIENTS        age public keys (age1…), space- or comma-separated
#   ARUGULA_DUMP_BUCKET, ARUGULA_DUMP_ENDPOINT, ARUGULA_DUMP_ACCESS_KEY_ID,
#   ARUGULA_DUMP_SECRET_ACCESS_KEY R2, else the app's LITESTREAM_* (the same bucket)
#   ARUGULA_DUMP_PREFIX            the objects' prefix (platform-pg)
#   ARUGULA_DUMP_KEEP_DAYS         prune dumps older than this (30)
#   ARUGULA_DUMP_KEEP_NEWEST       and never the newest this many (3)
#   ARUGULA_DUMP_EVERY_HOURS       skip while the newest dump is younger than
#                                  this many hours (20)
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
keep_days=${ARUGULA_DUMP_KEEP_DAYS:-30}
keep_newest=${ARUGULA_DUMP_KEEP_NEWEST:-3}
every_hours=${ARUGULA_DUMP_EVERY_HOURS:-20}
step="checking the retention"
[[ $keep_days =~ ^[0-9]+$ && $keep_newest =~ ^[1-9][0-9]*$ ]] ||
  { log "ARUGULA_DUMP_KEEP_DAYS and ARUGULA_DUMP_KEEP_NEWEST are whole numbers (the newest at least 1)"; exit 2; }
[[ $every_hours =~ ^[0-9]+$ ]] ||
  { log "ARUGULA_DUMP_EVERY_HOURS is a whole number of hours (0 dumps every run)"; exit 2; }
health=${ARUGULA_DUMP_PLATFORM_URL:-http://arugula-platform.internal:7700/platform.json}
step="checking the recipients"
read -r -a recipients <<<"$(tr ',' ' ' <<<"${ARUGULA_DUMP_RECIPIENTS:-}")"
[ "${#recipients[@]}" -gt 0 ] || { log "no ARUGULA_DUMP_RECIPIENTS (age public keys)"; exit 2; }
age_args=()
for r in "${recipients[@]}"; do
  case "$r" in age1*) age_args+=(-r "$r") ;; *) log "not an age public key: $r"; exit 2 ;; esac
done

work=$(mktemp -d)

# R2 with SigV4; the credentials go to curl on stdin, not its arguments.
s3() {
  printf 'user = "%s:%s"\n' "$key_id" "$secret" |
    curl -fsS -K - --retry 3 -m 600 --aws-sigv4 "aws:amz:$region:s3" "$@"
}
empty_sha=$(printf '' | sha256sum | cut -d' ' -f1)

# The gate: the machine starts every hour, and a run dumps only once the
# newest dump (latest.json's `at`, its name's time) is ARUGULA_DUMP_EVERY_HOURS
# old. Anything that doesn't give a time (no latest.json, R2 unreachable, a
# record without `at`, a time ahead of this clock) means dump: a dump too
# many is cheap, a missed one isn't. date -d takes "YYYY-MM-DD hh:mm:ss" in
# busybox (the image's) and GNU alike.
step="reading the newest dump's time"
if [ "$every_hours" -gt 0 ] &&
  s3 -m 30 -o "$work/latest.json" -H "x-amz-content-sha256: $empty_sha" \
    "${endpoint%/}/$bucket/$prefix/latest.json" 2>/dev/null; then
  last=$(jq -r '.at // empty' "$work/latest.json" 2>/dev/null || true)
  if [[ $last =~ ^([0-9]{4}-[0-9]{2}-[0-9]{2})T([0-9]{2})([0-9]{2})([0-9]{2})Z$ ]] &&
    then_s=$(date -u -d "${BASH_REMATCH[1]} ${BASH_REMATCH[2]}:${BASH_REMATCH[3]}:${BASH_REMATCH[4]}" +%s 2>/dev/null); then
    age=$(($(date +%s) - then_s))
    if [ "$age" -ge 0 ] && [ "$age" -lt $((every_hours * 3600)) ]; then
      log "skipped: the newest dump is $((age / 3600))h old ($last; one is due at ${every_hours}h)"
      ping "" "skipped: the newest dump, $last, is $((age / 3600))h old"
      exit 0
    fi
    [ "$age" -ge 0 ] || log "warning: the newest dump's time, $last, is ahead of this clock: dumping"
  else
    log "warning: latest.json gave no dump time ($last): dumping"
  fi
else
  [ "$every_hours" -eq 0 ] || log "warning: couldn't read $prefix/latest.json: dumping"
fi

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

# Retention, by the job itself (no lifecycle rule: nothing here can set one).
# A dump's time is in its name, so the age is the name's, not R2's clock.
# Every command checks its own result: in a function called from `if`,
# `set -e` doesn't stop it.
prune() {
  local listing="$work/list.xml" token="" keys="$work/keys" stems cutoff n=0 kept old q
  : > "$keys"
  while :; do
    q="list-type=2&prefix=${prefix}%2F"
    [ -z "$token" ] || q="$q&continuation-token=$(jq -rn --arg t "$token" '$t | @uri')"
    s3 -o "$listing" -H "x-amz-content-sha256: $empty_sha" \
      "${endpoint%/}/$bucket?$q" || return 1
    grep -o '<Key>[^<]*</Key>' "$listing" | sed 's/<[^>]*>//g' >> "$keys" || true
    grep -q '<IsTruncated>true</IsTruncated>' "$listing" || break
    token=$(grep -o '<NextContinuationToken>[^<]*</NextContinuationToken>' "$listing" | sed 's/<[^>]*>//g') || return 1
    [ -n "$token" ] || return 1
  done
  # Each dump's stem (prefix/time), newest first; latest.json isn't one.
  stems=$(grep -E "^$prefix/[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{6}Z\.(pgdump\.age|json)\$" "$keys" |
    sed -E 's/\.(pgdump\.age|json)$//' | sort -ru) || true
  cutoff="$prefix/$(date -u -d "@$(($(date +%s) - keep_days * 86400))" +%Y-%m-%dT%H%M%SZ)" || return 1
  kept=$(printf '%s\n' "$stems" | grep -c . || true)
  old=$(printf '%s\n' "$stems" | tail -n +"$((keep_newest + 1))" | awk -v c="$cutoff" 'NF && $0 < c')
  for stem in $old; do
    for ext in pgdump.age json; do
      if grep -qx "$stem.$ext" "$keys"; then
        s3 -o /dev/null -X DELETE -H "x-amz-content-sha256: $empty_sha" \
          "${endpoint%/}/$bucket/$stem.$ext" || return 1
      fi
    done
    n=$((n + 1))
  done
  log "pruned $n dumps older than $keep_days days; $((kept - n)) kept (never fewer than the newest $keep_newest)"
}
step="pruning"
if ! prune; then
  log "warning: pruning old dumps failed; this run's dump is in, and the next run tries again"
fi
ping "" "$object $bytes bytes $rows rows"
