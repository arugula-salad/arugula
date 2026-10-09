#!/bin/sh
# (Re)start the S34 rig in STATE_DIR (default /tmp/s34-rig), detached, and
# offer ale's fixer on ale-box. Stop it with: pkill -f "rig.ts $STATE"
set -e
STATE=${1:-/tmp/s34-rig}
HERE=$(cd "$(dirname "$0")/../.." && pwd)
pkill -f "rig.ts $STATE" 2>/dev/null || true
sleep 1
rm -f "$STATE/rig.json"
TARGET_DIR=$HERE/target/debug setsid nohup node --experimental-strip-types --no-warnings "$HERE/spikes/s34-agents-a2a/rig.ts" "$STATE" > "$STATE.log" 2>&1 &
until [ -f "$STATE/rig.json" ]; do sleep 1; done
( . "$STATE/ale.env"; illogical --host ale-box a2a offer fixer --dir "$STATE/ale/calc" > /dev/null )
echo "rig up in $STATE (log $STATE.log); fixer offered on ale-box"
