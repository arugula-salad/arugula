#!/bin/bash
# S36's scripted run (the rig must be up, with the conversation made by
# setup.sh): both repos reset to the failing test; "@lead" in the
# conversation on box-a; every permission card from L2 (box-b) answered in
# the conversation; done when the lead has reported after L2's turn ended.
# Prints the timeline as it grows.
set -u
STATE=$1
HERE=$(cd "$(dirname "$0")" && pwd)
c() { "$HERE/c.sh" "$STATE" "$@"; }
CONV=$(cat "$STATE/conv")
t0=$(date +%s.%N)
el() { printf '%6.1fs ' "$(echo "$(date +%s.%N) - $t0" | bc)"; }

for m in box-a box-b; do git -C "$STATE/$m/work/calc" checkout -q calc.py; done
seen=$(c box-a GET "/api/threads/conv-$CONV" | jq '.messages[-1].id // 0')
el; echo "@lead, from the owner"
c box-a POST "/api/threads/conv-$CONV" '{"text":"@lead have L1 and L2 each run python3 -m pytest -q and fix what fails, both at once, then tell me what they changed."}' | jq -c '{posted: .message.id, delivered: .agent.delivered}'
l2_ended=0
for _ in $(seq 1 120); do
  sleep 2
  while read -r m; do
    [ -z "$m" ] && continue
    id=$(echo "$m" | jq .id); name=$(echo "$m" | jq -r .name); text=$(echo "$m" | jq -r .text)
    seen=$id
    el; echo "#$id $name: ${text:0:300}"
    case "$name $text" in
      "L2 "*"needs input:"*) el; echo "  owner answers L2 in the conversation: $(c box-a POST "/api/convs/$CONV/act" '{"link":"L2","action":"allow"}' | jq -c .)";;
      "L2 "*"turn ended"*) l2_ended=1;;
      *"(agent)"*) [ $l2_ended = 1 ] && { el; echo done; exit 0; };;
    esac
  done < <(c box-a GET "/api/threads/conv-$CONV" | jq -c ".messages[] | select(.id > $seen)")
done
el; echo "timed out"; exit 1
