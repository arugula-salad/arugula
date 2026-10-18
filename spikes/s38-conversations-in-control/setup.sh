#!/bin/bash
# After rig.ts is up: Claude Code (haiku) in a pane on box-b, an M81
# conversation made on box-a (its lead starts there), box-b's grant to
# box-a for its pane, the pane linked as L1, and one message said before
# the conversation moves into control. Writes its id to STATE/conv.
set -eu
STATE=$1
HERE=$(cd "$(dirname "$0")" && pwd)
c() { "$HERE/c.sh" "$STATE" "$@"; }
B=$(c box-b POST /api/run "{\"command\":\"claude --model haiku --permission-mode default\",\"cwd\":\"$STATE/box-b/work/calc\",\"session\":\"work\"}" | jq -r .pane)
sleep 6
# The folder trust prompt, the first time Claude Code sees a folder.
if c box-b GET "/api/panes/$B/capture" | grep -q "trust this folder"; then
  c box-b POST "/api/panes/$B/keys" '{"keys":["Down","Enter"]}' > /dev/null
fi
sleep 4
CONV=$(c box-a POST /api/convs '{"title":"pineapple calc"}' | jq -r .id)
echo "$CONV" > "$STATE/conv"
echo "conversation $CONV, made on box-a; L1 = box-b %$B"
A_ID=$(jq -r '.machines["box-a"].id' "$STATE/rig.json")
B_ID=$(jq -r '.machines["box-b"].id' "$STATE/rig.json")
c box-b POST "/api/panes/$B/conv-grants" "{\"device\":\"$A_ID\",\"conv\":\"$CONV\"}" > /dev/null
# box-a trusts box-b once its trusted devices refresh (about two minutes
# after box-b joined, on this rig).
until r=$(c box-a POST "/api/convs/$CONV/links" "{\"machine\":\"$B_ID\",\"pane\":$B,\"title\":\"claude in calc\"}") && echo "$r" | jq -e .links > /dev/null; do
  echo "  ${r:0:100}"; sleep 10
done
echo "$r" | jq -c '[.links[] | {name, machine_name, pane}]'
c box-a POST "/api/threads/conv-$CONV" '{"text":"Said before the move: the word for today is pineapple."}' | jq -c '{said: .message.id}'
