#!/usr/bin/env bash
#
# M141's window, rehearsed on the testnet (docs/control-ops.md, "The first
# window"): the runbook's steps in order, against the `control` profile with
# the platform beside it (window.yaml), then both rollbacks.
#
#   testnet/window.sh        bring the stack up, rehearse, leave it up
#   just testnet down        afterwards
#
# What it does, as production will:
#   1. up: control with its platform in process, and the out-of-process
#      platform deployed beside it, held (ARUGULA_PLATFORM_PAUSED) on an
#      empty database
#   2. a person with chat rooms, messages, a read mark, and a project's
#      environment with a secret set (window.ts seed)
#   3. the notice, a day ahead (arugula-control notice post)
#   4. pre-checks: a trial copy, carried to the platform, whose secrets open
#      with the platform's key
#   5. the window: maintenance on with --pause, copy on the hub, carry it,
#      copy on the platform, the hub's tables unchanged since, secrets
#      open; flip control to the platform out of process, then let the
#      platform go; maintenance off
#   6. post-checks: everything reads back; the `platform` claim
#   7. rollback within the window: back in process on the hub's copies,
#      which the out-of-process time didn't touch
#   8. the window again over the platform's earlier copy (--replace), then
#      new work after it: a message, and the secret set again, which the
#      platform seals with the project's key it opens with the master key
#   9. rollback after the window: the platform's file copied back over the
#      hub's copies, then in process, with that new work there
#
# A transcript is kept in $STATE/window/ (each step's output), and the
# summary is printed at the end. Docker cp stands in for `fly ssh sftp`.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
# shellcheck source-path=SCRIPTDIR source=env.sh
. "$HERE/env.sh"
need_docker

W="$STATE/window"
export ARUGULA_TESTNET_UID ARUGULA_TESTNET_GID
ARUGULA_TESTNET_UID="$(id -u)"
ARUGULA_TESTNET_GID="$(id -g)"
# Where control's platform runs, and whether the out-of-process one is held.
export ARUGULA_TESTNET_PLATFORM_URL="" ARUGULA_TESTNET_PLATFORM_PAUSED=1

log() { echo "[testnet window] $*" >&2; }
die() { log "FAIL: $*"; exit 1; }
step() { echo; echo "== $*"; echo "== $*" >> "$W/transcript.txt"; }
now() { date +%s.%N; }
since() { awk -v a="$1" -v b="$(now)" 'BEGIN { printf "%.1f", b - a }'; }
dc() { docker compose -f "$HERE/compose.yaml" -f "$HERE/window.yaml" --profile ssh --profile control --profile platform "$@"; }
hub() { docker exec "$TESTNET-control" /opt/arugula/arugula-control "$@"; }
plat() { docker exec "$TESTNET-platform" /opt/arugula/arugula-platform "$@"; }
win() { node --experimental-strip-types --no-warnings "$HERE/window.ts" --state "$W/device.json" "$@"; }
claim() { "$HERE/test.sh" control "$@"; }
# The checksum column of a copy's or checksums' output, by table.
sums() { awk 'NF == 3 && length($3) == 64 { print $1, $3 }' "$1" | sort; }
# The copy's "to" checksums (a copy prints 16 hex characters of each).
copied() { awk 'NF == 6 && $NF == "ok" { print $1, $5 }' "$1" | sort; }
first16() { awk '{ print $1, substr($2, 1, 16) }'; }
record() { tee -a "$W/transcript.txt"; }
# Whether a container's log has TEXT.
logged() { docker logs "$1" > "$W/log.txt" 2>&1; grep -q "$2" "$W/log.txt"; }
answers() { curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$cport$1"; }

# Control (re)started with the platform where ARUGULA_TESTNET_PLATFORM_URL
# says, answering.
control_up() {
  dc up -d --wait control >/dev/null 2>&1 || dc up -d control
  for _ in $(seq 1 60); do
    curl -fsS "http://127.0.0.1:$cport/control.json" >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  docker logs --tail 30 "$TESTNET-control" >&2
  die "control didn't answer"
}
platform_up() {
  dc up -d --wait platform >/dev/null 2>&1 || dc up -d platform
  for _ in $(seq 1 60); do
    docker exec "$TESTNET-platform" bash -c 'exec 3<>/dev/tcp/127.0.0.1/7700' 2>/dev/null && return 0
    sleep 0.5
  done
  docker logs --tail 30 "$TESTNET-platform" >&2
  die "the platform didn't listen"
}
# Carry a file from one container's /window to the other's: what `fly ssh
# sftp get` then `put` do in production.
carry() {
  docker cp -q "$1:/window/$3" "$W/$3"
  docker cp -q "$W/$3" "$2:/window/$3"
  rm -f "$W/$3"
}
must_503() {
  local code
  code="$(answers /api/chat)"
  [ "$code" = 503 ] || die "GET /api/chat answered $code during the window, not 503"
}

# ---- 1. up
rm -rf "$W"
mkdir -p "$W/hub" "$W/platform"
: > "$W/transcript.txt"
step "1. up: control in process, the platform beside it, held"
ARUGULA_TESTNET_PLATFORM=in "$HERE/up.sh" control
# shellcheck source=/dev/null
. "$STATE/control.env"
cport="$ARUGULA_TESTNET_CONTROL_PORT"
dc up -d --wait fakes >/dev/null 2>&1 || dc up -d fakes
control_up
platform_up
logged "$TESTNET-platform" 'paused: no job worker' || die "the platform isn't held"
echo "control in process, platform held on an empty file" | record

# ---- 2. seed
step "2. a person's chat and secrets, before the window"
# shellcheck disable=SC2086 # CONTROL_VIA is several words
node --experimental-strip-types --no-warnings "$ROOT/web/fixtures/device-cli.ts" --state "$W/device.json" \
  signin --control "$CONTROL_URL" $CONTROL_VIA --login "window-$$" > /dev/null
win seed > "$W/seed.json"
record < "$W/seed.json"
win check "$W/seed.json" | record

# ---- 3. the notice
step "3. the notice, a day ahead"
title="Chat and Cloud maintenance"
body="Chat and Cloud will be down for a few minutes. Terminals keep working. After it, reload any control page you have open."
hub notice post --db /window/control.db --title "$title" --body "$body" | record
win notice "$title" | record

# ---- 4. pre-checks
step "4. pre-checks: a trial copy, and the platform's key opens its secrets"
hub platform secrets-check --db /window/control.db | record
hub platform copy --from /window/control.db --to /window/trial.db > "$W/trial.txt"
carry "$TESTNET-control" "$TESTNET-platform" trial.db
plat secrets-check --db /window/trial.db | record
docker exec "$TESTNET-control" rm -f /window/trial.db /window/trial.db-wal /window/trial.db-shm
docker exec "$TESTNET-platform" rm -f /window/trial.db /window/trial.db-wal /window/trial.db-shm
echo "trial copy: $(tail -n1 "$W/trial.txt")" | record

# ---- 5. the window
window() {
  local n="$1" replace="${2:-}" t0
  t0="$(now)"
  step "5.$n the window"
  hub maintenance on --retry-after 30 --pause --db /window/control.db | record
  sleep 2
  must_503
  [ "$(answers /control.json)" = 200 ] || die "the hub's own routes went down"
  echo "GET /api/chat: 503 Retry-After 30; GET /control.json: 200" | record
  hub platform copy --from /window/control.db --to /window/window.db > "$W/copy-hub-$n.txt"
  record < "$W/copy-hub-$n.txt"
  carry "$TESTNET-control" "$TESTNET-platform" window.db
  # shellcheck disable=SC2086 # --replace or nothing
  plat copy --from /window/window.db --to /window/platform.db $replace > "$W/copy-platform-$n.txt"
  record < "$W/copy-platform-$n.txt"
  diff <(copied "$W/copy-hub-$n.txt") <(copied "$W/copy-platform-$n.txt") || die "the two copies' checksums differ"
  hub platform checksums --db /window/control.db > "$W/hub-after-copy-$n.txt"
  diff <(copied "$W/copy-hub-$n.txt") <(sums "$W/hub-after-copy-$n.txt" | first16) \
    || die "the hub's tables changed after the copy"
  echo "checksums: the hub's copy = the platform's copy = the hub's tables now" | record
  plat secrets-check --db /window/platform.db | record
  docker exec "$TESTNET-platform" rm -f /window/window.db
  docker exec "$TESTNET-control" rm -f /window/window.db /window/window.db-wal /window/window.db-shm
  local t1; t1="$(now)"
  ARUGULA_TESTNET_PLATFORM_URL=http://platform:7700
  control_up
  hub maintenance status --db /window/control.db | record
  must_503
  ARUGULA_TESTNET_PLATFORM_PAUSED=0
  platform_up
  logged "$TESTNET-control" 'the platform runs out of process' || die "control isn't the gateway"
  echo "flipped: control is the gateway, the platform is running" | record
  hub maintenance off --db /window/control.db | record
  sleep 2
  echo "window $n: $(since "$t0") s of 503s (copy and checks $(awk -v a="$t0" -v b="$t1" 'BEGIN { printf "%.1f", b - a }') s)" | record
}

window 1
step "6.1 post-checks, out of process"
win check "$W/seed.json" | record
claim platform | record

# ---- 7. rollback within the window: the hub's copies, untouched
step "7. rollback within the window: back in process on the hub's copies"
hub platform checksums --db /window/control.db > "$W/hub-rollback-1.txt"
diff <(copied "$W/copy-hub-1.txt") <(sums "$W/hub-rollback-1.txt" | first16) \
  || die "the hub's copies changed while the platform ran out of process"
echo "the hub's copies: unchanged since the window's copy" | record
hub maintenance on --retry-after 30 --pause --db /window/control.db | record
ARUGULA_TESTNET_PLATFORM_PAUSED=1
platform_up
ARUGULA_TESTNET_PLATFORM_URL=""
control_up
logged "$TESTNET-control" 'the platform runs in process' || die "control isn't in process"
hub maintenance off --db /window/control.db | record
sleep 2
win check "$W/seed.json" | record
claim platform | record

# ---- 8. the window again, then new work
window 2 --replace
step "6.2 post-checks, and new work after the window"
win check "$W/seed.json" | record
win write flip "$W/seed.json" > "$W/write.json"
record < "$W/write.json"
plat secrets-check --db /window/platform.db > "$W/secrets-after.txt"
record < "$W/secrets-after.txt"
grep -q '^  1 open$' "$W/secrets-after.txt" || die "the secret set after the window doesn't open with the platform's key"
echo "the secret set after the window, sealed by the platform with the project's key: it opens" | record
claim platform | record

# ---- 9. rollback after the window: copy back
step "9. rollback after the window: the platform's file back into the hub's"
hub maintenance on --retry-after 30 --pause --db /window/control.db | record
ARUGULA_TESTNET_PLATFORM_PAUSED=1
platform_up
plat copy --from /window/platform.db --to /window/back.db > "$W/back-platform.txt"
carry "$TESTNET-platform" "$TESTNET-control" back.db
hub platform copy --from /window/back.db --to /window/control.db --replace > "$W/back-hub.txt"
record < "$W/back-hub.txt"
diff <(copied "$W/back-platform.txt") <(copied "$W/back-hub.txt") || die "the copy back differs"
ARUGULA_TESTNET_PLATFORM_URL=""
control_up
hub maintenance off --db /window/control.db | record
sleep 2
win check --has "after the window: flip" "$W/seed.json" | record
claim platform | record

step "done"
grep -h '^window [0-9]' "$W/transcript.txt" || true
log "transcript: $W/transcript.txt; the stack is still up: just testnet down"
