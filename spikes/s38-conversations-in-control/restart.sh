#!/bin/bash
# restart.sh STATE MACHINE: stop that rig machine's daemon by its exact PID
# and start it again on the same state dir and port, detached, in a
# stripped environment (leads on haiku). Prints the old and new PID.
set -eu
STATE=$1 M=$2
TARGET=$(jq -r .target "$STATE/rig.json")
port=$(jq -r ".machines[\"$M\"].port" "$STATE/rig.json")
dir=$(jq -r ".machines[\"$M\"].state" "$STATE/rig.json")
pidf="$STATE/$M.pid"
old=$(cat "$pidf" 2>/dev/null || jq -r ".machines[\"$M\"].pid" "$STATE/rig.json")
ps -o pid=,args= -p "$old" | grep -q "arugulad --listen 127.0.0.1:$port " || { echo "pid $old isn't $M's daemon"; exit 1; }
echo "old: $old"
kill -CONT "$old" 2>/dev/null || true
kill "$old"
while kill -0 "$old" 2>/dev/null; do sleep 0.2; done
NODE_DIR=$(dirname "$(command -v node)")
env -i HOME="$HOME" USER="$USER" PATH="$TARGET:$NODE_DIR:/usr/local/bin:/usr/bin:/bin:$HOME/.local/bin" \
  XDG_RUNTIME_DIR="${XDG_RUNTIME_DIR:-}" RUST_LOG=arugulad=info S38_LEAD_MODEL=haiku \
  setsid nohup "$TARGET/arugulad" --listen "127.0.0.1:$port" --name "$M" --state-dir "$dir" \
  --shell "bash --norc --noprofile" --no-manager-env --tailscale-socket /nonexistent --no-claude-ide \
  >> "$STATE/$M.log" 2>&1 &
new=$!
echo "$new" > "$pidf"
until curl -s -o /dev/null "http://127.0.0.1:$port/"; do sleep 0.2; done
echo "new: $new"
