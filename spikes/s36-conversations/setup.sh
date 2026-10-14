#!/bin/bash
# After rig.ts is up: Claude Code (haiku) in a pane on each machine (box-b's
# in default permission mode, so it asks), a conversation on box-a, box-b's
# grant to box-a for its pane, both panes linked, and the lead started.
# Writes the conversation's id to STATE/conv.
set -eu
STATE=$1
HERE=$(cd "$(dirname "$0")" && pwd)
c() { "$HERE/c.sh" "$STATE" "$@"; }
pane_of() { jq -r .pane; }
A=$(c box-a POST /api/run "{\"command\":\"claude --model haiku\",\"cwd\":\"$STATE/box-a/work/calc\",\"session\":\"work\"}" | pane_of)
B=$(c box-b POST /api/run "{\"command\":\"claude --model haiku --permission-mode default\",\"cwd\":\"$STATE/box-b/work/calc\",\"session\":\"work\"}" | pane_of)
sleep 6
# The folder trust prompt, the first time Claude Code sees a folder.
for mp in "box-a $A" "box-b $B"; do
  set -- $mp
  if c "$1" GET "/api/panes/$2/capture" | grep -q "trust this folder"; then
    c "$1" POST "/api/panes/$2/keys" '{"keys":["Down","Enter"]}' > /dev/null
  fi
done
sleep 4
CONV=$(c box-a POST /api/convs '{"title":"fix calc on both boxes"}' | jq -r .id)
echo "$CONV" > "$STATE/conv"
echo "conversation $CONV; L1 = box-a %$A, L2 = box-b %$B"
echo "linking box-b's pane before its grant:"
c box-a POST "/api/convs/$CONV/links" "{\"machine\":\"box-b\",\"pane\":$B,\"title\":\"claude in calc\"}"
A_ID=$(jq -r '.machines["box-a"].id' "$STATE/rig.json")
c box-b POST /api/conv-grants "{\"device\":\"$A_ID\",\"pane\":$B,\"conv\":\"$CONV\"}" > /dev/null
c box-a POST "/api/convs/$CONV/links" "{\"pane\":$A,\"title\":\"claude in calc\"}" > /dev/null
# box-a trusts box-b once its trusted devices refresh (about two minutes
# after box-b joined, on this rig).
until r=$(c box-a POST "/api/convs/$CONV/links" "{\"machine\":\"box-b\",\"pane\":$B,\"title\":\"claude in calc\"}") && echo "$r" | jq -e .links > /dev/null; do
  echo "  ${r:0:100}"; sleep 10
done
echo "$r" | jq -c .links
c box-a POST "/api/convs/$CONV/lead" "{\"cwd\":\"$STATE/box-a/work\",\"model\":\"haiku\"}" | jq -c '{lead}'
until c box-a GET "/api/convs/$CONV" | jq -r .lead | xargs -I{} "$HERE/c.sh" "$STATE" box-a GET /api/blocks/{} | jq -e '.state.attention == "done"' > /dev/null; do sleep 2; done
echo "lead ready"
