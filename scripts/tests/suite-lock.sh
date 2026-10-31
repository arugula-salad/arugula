#!/usr/bin/env bash
# Tests scripts/suite-lock: COMMAND's exit code comes back; a second run
# waits for the first and says who holds the lock; a run inside a locked
# one, CI and ARUGULA_SUITE_LOCK=0 don't wait; a holder killed with -9
# leaves no lock behind; ARUGULA_SUITE_LOCK_FILE is one lock for every
# home; SIGTERM reaches COMMAND; a syspolicyd that stays
# busy is refused, with the lock left free, and one that settles isn't;
# a run past SUITE_LOCK_TIMEOUT is stopped with its children (one that
# ignores TERM too), exit 142, and the next waiter gets the lock; a waiter
# is told every so often who holds it and for how long; `just e2e-spec`
# hands `-g "two words"` to Playwright as one argument.
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

# ARUGULA_SUITE_LOCK_FILE: users with homes of their own (a dev box's
# session users) share one lock.
shared=$work/box/suite.lock
mkdir -p "$work/box"
touch "$work/go5"
HOME=$work/other ARUGULA_SUITE_LOCK_FILE=$shared "$lock" sh -c "touch '$work/held5'; while [ -e '$work/go5' ]; do sleep 0.05; done" &
pids+=($!)
until_file "$work/held5"
if [ ! -e "$work/other/.cache/arugula/suite.lock" ] && [ -e "$shared" ]; then ok "ARUGULA_SUITE_LOCK_FILE is the lock"; else bad "the lock isn't $shared"; fi
start=$SECONDS
(sleep 1; rm -f "$work/go5") &
ARUGULA_SUITE_LOCK_FILE=$shared "$lock" true 2>/dev/null
if [ $((SECONDS - start)) -ge 1 ]; then ok "another home waits on the shared lock"; else bad "another home didn't wait on $shared"; fi
wait

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

# A run past its limit: the command and its child (which ignores TERM) are
# stopped, the exit is 142, and a second waiter takes the lock after.
rm -f "$work/kid" "$work/ran"
cat >"$work/stubborn.sh" <<STUB
#!/bin/sh
sh -c "trap '' TERM; echo \$\$ >'$work/kid'; while :; do sleep 0.1; done" &
echo \$! >"$work/lead"
while :; do sleep 0.1; done
STUB
chmod +x "$work/stubborn.sh"
set +e
SUITE_LOCK_TIMEOUT=2 "$lock" "$work/stubborn.sh" >"$work/out" 2>&1 &
runner=$!
pids+=("$runner")
until_file "$work/kid"
kid=$(cat "$work/kid")
pids+=("$kid")
start=$SECONDS
out2=$("$lock" sh -c "touch '$work/ran'" 2>&1)
waited=$((SECONDS - start))
wait "$runner"
code=$?
set -e
if [ "$code" = 142 ]; then ok "a run past its limit exits 142"; else bad "past the limit: wanted 142, got $code: $(cat "$work/out")"; fi
case "$(cat "$work/out")" in *"ran past 2s; stopped it and its children") ok "and says so" ;; *) bad "no limit line: $(cat "$work/out")" ;; esac
if ! kill -0 "$kid" 2>/dev/null && ! kill -0 "$(cat "$work/lead")" 2>/dev/null; then ok "the command and a child that ignores TERM are gone"; else bad "a process outlived the limit"; fi
if [ -e "$work/ran" ] && [ "$waited" -ge 1 ]; then ok "a second waiter gets the lock after"; else bad "the waiter didn't run after the limit: $out2"; fi
# The waiter above waited about 2 s, and its own limit (default) hadn't
# started: it ran. A run inside its limit exits as its command did.
if SUITE_LOCK_TIMEOUT=3 "$lock" sh -c 'exit 0'; then ok "a run inside its limit finishes normally"; else bad "a run inside its limit failed"; fi

# Waiting says who holds the lock and for how long, again every so often.
touch "$work/go6"
hold "$work/go6"
(sleep 3; rm -f "$work/go6") &
out=$(ARUGULA_SUITE_LOCK_NOTE=1 "$lock" true 2>&1)
case "$out" in *"waiting for pid "*" in "*" running sh -c "*", held for "*) ok "a waiter names the holder's worktree and command, and how long" ;; *) bad "no holder detail: $out" ;; esac
if [ "$(printf '%s\n' "$out" | grep -c 'waiting for')" -ge 2 ]; then ok "and says it again while it waits"; else bad "said once: $out"; fi
wait

# `just e2e-spec e2e/x.spec.ts -g "two words"` (its dependencies, the
# builds, skipped) hands Playwright one -g pattern. A stand-in pnpm lists
# its arguments.
mkdir -p "$work/bin"
# shellcheck disable=SC2016 # the stand-in expands it
printf '#!/bin/sh\nfor a; do echo "arg:$a"; done\n' >"$work/bin/pnpm"
chmod +x "$work/bin/pnpm"
args=$(cd "$root" && PATH="$work/bin:$PATH" E2E_PORT=1 ARUGULA_SUITE_LOCK=0 just --no-deps e2e-spec e2e/x.spec.ts -g "two words" 2>/dev/null || true)
want=$(printf 'arg:exec\narg:playwright\narg:test\narg:e2e/x.spec.ts\narg:-g\narg:two words')
if [ "$args" = "$want" ]; then ok "e2e-spec keeps -g \"two words\" one pattern"; else bad "e2e-spec arguments: $args"; fi

exit $fail
