#!/bin/bash
# S38's scripted run (rig.ts up, setup.sh done). In order:
#  1. box-a moves its conversation into control (it takes the lease first);
#     box-b mirrors it.
#  2. @lead from box-a: box-a's lead prompts L1 (on box-b, through the
#     relay) and answers.
#  3. control's database, dumped: no message text in it.
#  4. box-a "sleeps" (SIGSTOP). @lead from box-b: box-b takes the lease once
#     box-a's expires, starts a lead, prompts L1 and answers.
#  5. box-a wakes (SIGCONT): it finds it lost the lease and closes its lead.
#     A lead's post with its old token is refused by control.
#  6. box-a restarts: it mirrors, doesn't lead, and its own @lead (a
#     person's) is answered by box-b's lead, which knows what was said
#     before the move.
#  7. Who appended what, from control's log and a member's reading of it.
set -u
STATE=$1
HERE=$(cd "$(dirname "$0")" && pwd)
c() { "$HERE/c.sh" "$STATE" "$@"; }
CONV=$(cat "$STATE/conv")
DB="$STATE/control/control.db"
t0=$(date +%s.%N)
el() { printf '%6.1fs ' "$(echo "$(date +%s.%N) - $t0" | bc)"; }
last() { c "$1" GET "/api/threads/conv-$CONV" | jq '.messages[-1].id // 0'; }
leads() { for m in box-a box-b; do printf '%s: %s  ' "$m" "$(c $m GET /api/s38/convs/$CONV 2>/dev/null | jq -c '{leading, lead_block}' 2>/dev/null)"; done; echo; }
# wait_reply MACHINE AFTER SECS: until a lead's reply after message AFTER.
wait_reply() {
  local m=$1 after=$2 secs=$3 seen=$2
  for _ in $(seq 1 "$secs"); do
    while read -r msg; do
      [ -z "$msg" ] && continue
      seen=$(echo "$msg" | jq .id)
      el; echo "  #$seen $(echo "$msg" | jq -r .name): $(echo "$msg" | jq -r .text | tr '\n' ' ' | cut -c1-200)"
      if [ "$(echo "$msg" | jq -r '.who == "lead" and .event == null')" = true ]; then return 0; fi
    done < <(c "$m" GET "/api/threads/conv-$CONV" | jq -c ".messages[] | select(.id > $seen)")
    sleep 1
  done
  el; echo "  no reply within ${secs}s"; return 1
}
pid_of() { cat "$STATE/$1.pid" 2>/dev/null || jq -r ".machines[\"$1\"].pid" "$STATE/rig.json"; }

el; echo "1. box-a moves conversation $CONV into control"
c box-a POST "/api/s38/convs/$CONV/upload" | jq -c .
for _ in $(seq 1 30); do
  [ "$(last box-b)" = "$(last box-a)" ] && c box-b GET /api/convs/$CONV/links | jq -e "length > 0" > /dev/null && break; sleep 1
done
el; echo "   box-b mirrors it: $(c box-b GET /api/s38/convs/$CONV | jq -c '{messages, leading}'); box-b sees the links: $(c box-b GET /api/convs/$CONV/links | jq -c .)"
leads

el; echo "2. @lead, from box-a"

a=$(c box-a POST "/api/threads/conv-$CONV" '{"text":"@lead ask L1 what 17 times 23 is, then tell me its answer."}' | jq .message.id); echo "{\"posted\":$a}"
wait_reply box-a "$a" 240
leads

el; echo "3. control's database, dumped"
python3 -I "$HERE/dump.py" "$DB" pineapple "times" "ask l1" "tell me"
echo "   (exit $?: 0 means none of those words is in it)"

el; echo "4. box-a sleeps (SIGSTOP $(pid_of box-a)); @lead from box-b"
kill -STOP "$(pid_of box-a)"
b=$(c box-b POST "/api/threads/conv-$CONV" '{"text":"@lead box-a is asleep. Ask L1 what 6 times 7 is, then tell me."}' | jq .message.id)
echo "{\"posted\":$b}"
for _ in $(seq 1 40); do
  tok=$(c box-b GET /api/s38/convs/$CONV | jq .leading)
  [ "$tok" != null ] && { el; echo "   box-b took the lease: token $tok"; break; }
  sleep 1
done
wait_reply box-b "$b" 300

el; echo "5. box-a wakes (SIGCONT)"
kill -CONT "$(pid_of box-a)"
sleep 8
leads
grep -E 's38: (lost|closed|not posting|control refused)' "$STATE/box-a.log" | tail -5 | sed 's/^/   box-a log: /'
el; echo "   a lead's post from box-a with its old token 1:"
c box-a POST "/api/s38/convs/$CONV/probe" '{"token":1,"text":"stale lead says hi"}'

el; echo "6. box-a restarts"
"$HERE/restart.sh" "$STATE" box-a
sleep 15
leads

a=$(c box-a POST "/api/threads/conv-$CONV" '{"text":"@lead what was the word for today, said before the move?"}' | jq .message.id); echo "{\"posted\":$a}"
wait_reply box-a "$a" 240
sleep 5
leads

el; echo "7. who appended each entry (control's half: seq and device), and what it says (a member's half)"
c box-b GET "/api/threads/conv-$CONV" > "$STATE/timeline.json"
python3 -I "$HERE/table.py" "$DB" "$STATE/timeline.json" "$STATE/rig.json"
python3 -I "$HERE/dump.py" "$DB" pineapple "times" "asleep" "word for today" "said before" "turn ended" > "$STATE/dump-final.txt"
echo "   the final dump holds none of the messages' words: $([ $? = 0 ] && echo yes || echo NO)"
