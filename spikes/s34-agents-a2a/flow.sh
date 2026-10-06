#!/bin/bash
# S34's scripted run (the rig must be up: up.sh): jake's CLI sends ale's
# fixer a task on ale-box through control; ale allows it (for an hour) and
# approves what the agent asks to run; jake gets the patch and applies it
# in his own clone, where the tests then pass.
set -u
STATE=${1:-/tmp/s34-rig}
HERE=$(cd "$(dirname "$0")" && pwd)
act() { node --experimental-strip-types --no-warnings "$HERE/act.ts" "$STATE" "$@"; }
as() { (who=$1; shift; . "$STATE/$who.env"; "$@"); }
t0=$(date +%s.%N)
el() { printf '%6.1fs ' "$(echo "$(date +%s.%N) - $t0" | bc)"; }

(cd "$STATE/jake/calc" && python3 -m pytest -q >/dev/null 2>&1; ls -d __pycache__)  # a test run leaves .pyc files, as in a real clone
el; echo "jake lists ale-box's agents:"
as jake illogical --host ale-box a2a agents
el; echo "jake sends a task"
T=$(as jake illogical --host ale-box --json a2a send fixer "test_calc.py fails. Fix calc.py so the tests pass." --no-wait 2>/dev/null | jq -r .id)
el; echo "task $T; ale's waiting list:"
as ale illogical --host ale-box a2a waiting
el; echo "jake tries to allow it himself:"
as jake illogical --host ale-box a2a answer "$T" allow 2>&1
el; echo "ale allows jake for an hour:"
as ale illogical --host ale-box a2a answer "$T" hour
last=""
while :; do
  s=$(as jake illogical --host ale-box --json a2a get fixer "$T")
  st=$(echo "$s" | jq -r .status.state); msg=$(echo "$s" | jq -r '.status.message.parts[0].text // ""')
  [ "$st $msg" != "$last" ] && { el; echo "$st $msg"; last="$st $msg"; }
  case $st in TASK_STATE_COMPLETED|TASK_STATE_FAILED|TASK_STATE_REJECTED|TASK_STATE_CANCELED|TASK_STATE_INPUT_REQUIRED) break;; esac
  case $msg in *approve*) el; echo "ale approves on ale-box: $(act ale ale-box allow)";; esac
  sleep 2
done
as jake illogical --host ale-box a2a get fixer "$T"
el; echo "jake applies the patch in his clone:"
cd "$STATE/jake/calc" && as jake illogical --host ale-box a2a patch fixer "$T" | git apply --3way && git status --short && python3 -m pytest -q 2>&1 | tail -2
el; echo done
