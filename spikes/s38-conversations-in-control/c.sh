#!/bin/bash
# c.sh STATE MACHINE METHOD PATH [JSON]: an API call to a rig machine, as
# its owner (its local token), straight to its port (not through control).
STATE=$1 M=$2 METHOD=$3 P=$4 BODY=${5:-}
url=$(jq -r ".machines[\"$M\"].url" "$STATE/rig.json")
tok=$(jq -r ".machines[\"$M\"].token" "$STATE/rig.json")
if [ -n "$BODY" ]; then
  curl -sS -X "$METHOD" -H "Authorization: Bearer $tok" -H 'content-type: application/json' -d "$BODY" "$url$P"
else
  curl -sS -X "$METHOD" -H "Authorization: Bearer $tok" "$url$P"
fi
echo
