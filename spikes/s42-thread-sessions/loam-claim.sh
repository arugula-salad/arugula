# S42 (prototype, not for main): a testnet claim, sourced into
# testnet/test.sh by rig.sh. A hosted workspace on loam, end to end:
# - q1: session-proto.sh on the box, in a pane, its output kept;
# - q4 (#124): the daemon killed with loam supervising it, then stopped
#   (as on E2B, where nothing restarts it), the box paused and woken.
# Needs LOAM_KEY, LOAM_PUBLIC (control's tunnel), S42_OUT (a directory).
# shellcheck shell=bash
claim_s42_loam() {
  local ok=1 fp team proj LH lpid code sbx t0 i
  local K="$LOAM_KEY" A=https://api.loam.inevitable.fyi E=https://api.e2b.loam.inevitable.fyi
  # shellcheck disable=SC2086
  fp="$(dev signin --control "$LOAM_PUBLIC" $CONTROL_VIA --login "s42-$$-$RANDOM" | field fingerprint)"
  [ -n "$fp" ] || { note "s42: no device signed in"; return 1; }
  team="$(dev team-create "s42 team" | field team)"
  proj="$(dev delegate "$team" | field project)"
  [ -n "$proj" ] || { note "s42: couldn't delegate"; return 1; }
  LH="$(mktemp -d)"
  bcli() { HOME="$LH" XDG_CONFIG_HOME="$LH/.config" XDG_STATE_HOME="$LH/.state" XDG_RUNTIME_DIR="$LH" ARUGULA_SOCK="$LH/none.sock" "$ARUGULA_TESTNET_BINARIES/arugula" "$@"; }
  bcli login "$LOAM_PUBLIC" --name hostcli --account "$fp" < /dev/null > "$WORK/s42-login" 2>&1 &
  lpid=$! code=""
  for _ in $(seq 1 60); do
    code="$(sed -nE 's/.*#join=([A-Z0-9]+-[A-Z0-9]+(\.[A-Z0-9-]+)?).*/\1/p' "$WORK/s42-login" | head -1)"
    [ -n "$code" ] && break; kill -0 "$lpid" 2>/dev/null || break; sleep 0.5
  done
  [ -n "$code" ] || { note "s42: login showed no code"; cat "$WORK/s42-login" >&2; return 1; }
  dev approve "$code" > /dev/null || { note "s42: approve failed"; return 1; }
  wait "$lpid"; grep -q "Logged in." "$WORK/s42-login" || { note "s42: login failed"; cat "$WORK/s42-login" >&2; return 1; }
  t0=$(date +%s)
  bcli cloud create dev --project "$proj" > "$WORK/s42-create" 2>&1 || { note "s42: create:"; cat "$WORK/s42-create" >&2; return 1; }
  bcli cloud open dev --project "$proj" --wait > "$WORK/s42-open" 2>&1 || { note "s42: open --wait:"; cat "$WORK/s42-open" >&2; ok=0; }
  note "s42: ready after $(( $(date +%s) - t0 )) s: $(tail -1 "$WORK/s42-open")"
  sbx="$(curl -s -H "X-API-Key: $K" "$E/v2/sandboxes?metadata=arugula_project%3D$proj" | sed -n 's/.*"sandboxID":"\([^"]*\)".*/\1/p' | head -1)"
  note "s42: box $sbx"

  # q1: a session by hand, on the box (S42_SKIP_Q1=1: not again).
  if [ -z "${S42_SKIP_Q1:-}" ]; then
  bcli run --host dev --wait 'bash /opt/s42/session-proto.sh > /home/user/s42-q1.txt 2>&1' > "$WORK/s42-q1run" 2>&1
  note "s42: q1 ran ($(tail -2 "$WORK/s42-q1run" | tr '\n' ' '))"
  bcli fs --host dev cat /home/user/s42-q1.txt > "$S42_OUT/q1-loam.txt" 2>&1 || ok=0
  note "s42: q1: $(wc -l < "$S42_OUT/q1-loam.txt") lines in $S42_OUT/q1-loam.txt"
  fi

  # q4a: the daemon killed, loam supervising it (its services restart).
  {
    echo "== q4a. arugulad killed (kill -9) on a box whose provider supervises it (loam's services: the template's start command is its service e2b-template-start)"
    echo "\$ service before: $(curl -s -H "Authorization: Bearer $K" "$A/v1/sandboxes/$sbx/services/e2b-template-start")"
    bcli run --host dev 'sudo -n pkill -9 -x arugulad' 2>&1 | head -2
    t0=$(date +%s); sleep 2
    for i in $(seq 1 60); do bcli ls --host dev > /dev/null 2>&1 && break; sleep 1; done
    echo "\$ reachable again through the relay after $(( $(date +%s) - t0 )) s (tries $i)"
    echo "\$ service after: $(curl -s -H "Authorization: Bearer $K" "$A/v1/sandboxes/$sbx/services/e2b-template-start")"
    echo "\$ cloud ls: $(bcli cloud ls --project "$proj" 2>&1 | tail -1)"

    echo; echo "== q4b. the daemon gone with nothing to restart it (E2B's case: loam's service stopped)"
    echo "\$ stop: $(curl -s -X POST -H "Authorization: Bearer $K" "$A/v1/sandboxes/$sbx/services/e2b-template-start/stop")"
    sleep 5
    echo "\$ cloud ls: $(bcli cloud ls --project "$proj" 2>&1 | tail -1)"
    echo "\$ pause at the provider (as the idle timeout would): $(curl -s -o /dev/null -w %{http_code} -X POST -H "X-API-Key: $K" -H 'content-type: application/json' -d '{}' "$E/sandboxes/$sbx/pause")"
    for _ in $(seq 1 30); do bcli cloud ls --project "$proj" 2>/dev/null | grep -q paused && break; sleep 2; done
    echo "\$ cloud ls: $(bcli cloud ls --project "$proj" 2>&1 | tail -1)"
    t0=$(date +%s)
    echo "\$ cloud open dev --wait:"; bcli cloud open dev --project "$proj" --wait 2>&1 | tail -3
    echo "(after $(( $(date +%s) - t0 )) s)"
    t0=$(date +%s)
    echo "\$ ls --host dev (opening the machine, which waits on its wake):"; bcli ls --host dev 2>&1 | tail -3
    echo "(after $(( $(date +%s) - t0 )) s)"
    echo "\$ provider: $(curl -s -H "X-API-Key: $K" "$E/sandboxes/$sbx" | sed -n 's/.*"state":"\([a-z]*\)".*/\1/p')"
    echo "\$ cloud ls: $(bcli cloud ls --project "$proj" 2>&1 | tail -1)"
    sleep 20
    echo "\$ cloud ls, 20 s on: $(bcli cloud ls --project "$proj" 2>&1 | tail -1)"
    echo "\$ service: $(curl -s -H "Authorization: Bearer $K" "$A/v1/sandboxes/$sbx/services/e2b-template-start")"

    echo; echo "== q4c. the daemon started again in the box (what a supervisor, or the platform through the provider, would do)"
    echo "\$ start: $(curl -s -X POST -H "Authorization: Bearer $K" "$A/v1/sandboxes/$sbx/services/e2b-template-start/start")"
    t0=$(date +%s)
    for i in $(seq 1 60); do bcli ls --host dev > /dev/null 2>&1 && break; sleep 1; done
    echo "\$ reachable again after $(( $(date +%s) - t0 )) s (tries $i)"
    echo "\$ cloud ls: $(bcli cloud ls --project "$proj" 2>&1 | tail -1)"
  } > "$S42_OUT/q4-loam.txt" 2>&1
  note "s42: q4: $(wc -l < "$S42_OUT/q4-loam.txt") lines in $S42_OUT/q4-loam.txt"
  docker logs "${COMPOSE_PROJECT_NAME:-arugula-testnet}-control" 2>&1 | grep -aE "hosted|wake|dial|woke" | grep -v DEBUG | tail -40 > "$S42_OUT/q4-control-log.txt"

  bcli cloud delete dev --project "$proj" > "$WORK/s42-delete" 2>&1 || { note "s42: delete:"; cat "$WORK/s42-delete" >&2; ok=0; }
  rm -rf "$LH"
  [ "$ok" = 1 ]
}
