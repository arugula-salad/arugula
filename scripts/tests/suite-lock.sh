#!/usr/bin/env bash
# Tests scripts/suite-lock: COMMAND's exit code comes back; a second run
# waits for the first and says who holds the lock; a run inside a locked
# one, CI and ARUGULA_SUITE_LOCK=0 don't wait; a holder killed with -9
# leaves no lock behind; SIGTERM reaches COMMAND; a syspolicyd that stays
# busy is refused, with the lock left free, and one that settles isn't.
# The lock lives under a temp HOME, and a stand-in ps plays syspolicyd.
#
#   scripts/tests/suite-lock.sh
set -euo pipefail

root=$(cd "$(dirname "$0")/../.." && pwd)
lock=$root/scripts/suite-lock
work=$(mktemp -d)
pids=()
# shellcheck disable=SC2329 # run by the trap
cleanup() {
  for p in "${pids[@]}"; do kill -9 "$p" 2>/dev/null || true; done
  rm -rf "$work"
}
trap cleanup EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

export HOME=$work/home
mkdir -p "$HOME"
unset CI ARUGULA_SUITE_LOCK ARUGULA_SUITE_LOCKED ARUGULA_SUITE_LOCK_BUSY

# Wait until `$1` (a file) exists: a background run got going.
until_file() { for _ in $(seq 1 100); do [ -e "$1" ] && return 0; sleep 0.05; done; return 1; }
# A run that holds the lock until `$1` is removed.
hold() {
  rm -f "$work/held"
  "$lock" sh -c "touch '$work/held'; while [ -e '$1' ]; do sleep 0.05; done" &
  pids+=($!)
  until_file "$work/held"
}

# The command's exit code is the script's.
set +e
"$lock" sh -c 'exit 7'
code=$?
set -e
if [ "$code" = 7 ]; then ok "exit code passed through"; else bad "exit code: wanted 7, got $code"; fi

# A second run waits for the first and names it.
touch "$work/go1"
hold "$work/go1"
start=$SECONDS
(sleep 1; rm -f "$work/go1") &
out=$("$lock" true 2>&1)
waited=$((SECONDS - start))
case "$out" in *"waiting for pid "*) ok "a second run says who holds the lock" ;; *) bad "no holder named: $out" ;; esac
if [ "$waited" -ge 1 ]; then ok "a second run waits"; else bad "a second run didn't wait ($waited s)"; fi
wait

# Inside a locked run, CI, and ARUGULA_SUITE_LOCK=0: no waiting.
touch "$work/go3"
hold "$work/go3"
quick() {
  local start=$SECONDS
  env "$@" "$lock" true >/dev/null 2>&1
  [ $((SECONDS - start)) -lt 2 ]
}
if quick ARUGULA_SUITE_LOCKED=1; then ok "a run inside a locked run doesn't wait"; else bad "a nested run waited"; fi
if quick CI=true; then ok "CI doesn't wait"; else bad "CI waited"; fi
if quick ARUGULA_SUITE_LOCK=0; then ok "ARUGULA_SUITE_LOCK=0 doesn't wait"; else bad "ARUGULA_SUITE_LOCK=0 waited"; fi
# And the real nesting: suite-lock inside COMMAND.
start=$SECONDS
rm -f "$work/go3"
wait
touch "$work/go4"
if "$lock" sh -c "'$lock' true" && [ $((SECONDS - start)) -lt 3 ]; then ok "suite-lock inside COMMAND runs at once"; else bad "suite-lock inside COMMAND waited on itself"; fi
rm -f "$work/go4"

# A holder killed with -9 (its command still running) frees the lock.
"$lock" sh -c "echo \$\$ >'$work/child'; sleep 30" &
holder=$!
pids+=("$holder")
until_file "$work/child"
pids+=("$(cat "$work/child")")
kill -9 "$holder"
wait "$holder" 2>/dev/null || true
if quick; then ok "a holder killed with -9 leaves no lock"; else bad "the lock outlived its holder"; fi

# SIGTERM to the script reaches COMMAND, whose exit code comes back.
"$lock" sh -c "trap 'echo got-term >\"$work/term\"; exit 3' TERM; touch '$work/up'; while :; do sleep 0.05; done" &
term=$!
pids+=("$term")
until_file "$work/up"
kill -TERM "$term"
set +e
wait "$term"
code=$?
set -e
if [ -e "$work/term" ]; then ok "SIGTERM reaches COMMAND"; else bad "COMMAND never saw SIGTERM"; fi
if [ "$code" = 3 ]; then ok "and its exit code comes back"; else bad "after SIGTERM: wanted 3, got $code"; fi

# A stand-in ps: syspolicyd at the CPU each line of $work/cpu says, one
# line per call, the last one repeating.
stubs=$work/stubs
mkdir -p "$stubs"
cat >"$stubs/ps" <<STUB
#!/bin/sh
n=\$(cat "$work/calls" 2>/dev/null || echo 0); n=\$((n + 1)); echo \$n >"$work/calls"
cpu=\$(sed -n "\${n}p" "$work/cpu"); [ -n "\$cpu" ] || cpu=\$(tail -n 1 "$work/cpu")
echo " %CPU COMM"
echo "  0.0 /sbin/launchd"
echo " \$cpu /usr/libexec/syspolicyd"
STUB
chmod +x "$stubs/ps"

# Busy the whole time: refused, with the fix, and the lock left free.
printf '99.0\n' >"$work/cpu"
rm -f "$work/calls"
set +e
out=$(PATH="$stubs:$PATH" ARUGULA_SUITE_LOCK_BUSY=2 "$lock" sh -c "touch '$work/ran'" 2>&1)
code=$?
set -e
if [ "$code" = 1 ] && [ ! -e "$work/ran" ]; then ok "a syspolicyd busy throughout is refused"; else bad "busy syspolicyd: exit $code, ran: $([ -e "$work/ran" ] && echo yes || echo no)"; fi
case "$out" in *"sudo killall syspolicyd"*) ok "and the refusal says how to fix it" ;; *) bad "no fix in: $out" ;; esac
if quick; then ok "a refused run leaves no lock"; else bad "a refused run kept the lock"; fi

# Busy for longer than a glance (four samples, about 6 s), then settling:
# it waits and runs.
printf '99.0\n99.0\n99.0\n99.0\n3.0\n' >"$work/cpu"
rm -f "$work/calls" "$work/ran"
out=$(PATH="$stubs:$PATH" ARUGULA_SUITE_LOCK_BUSY=30 "$lock" sh -c "touch '$work/ran'" 2>&1) || true
if [ -e "$work/ran" ]; then ok "a syspolicyd that settles waits, then runs"; else bad "a settling syspolicyd: $out"; fi
case "$out" in *"waiting up to 30s"*) ok "and says it's waiting" ;; *) bad "no wait said: $out" ;; esac

# Quiet: no word about it.
printf '1.0\n' >"$work/cpu"
rm -f "$work/calls"
out=$(PATH="$stubs:$PATH" "$lock" true 2>&1) || true
if [ -z "$out" ]; then ok "a quiet syspolicyd says nothing"; else bad "a quiet syspolicyd said: $out"; fi

exit $fail
