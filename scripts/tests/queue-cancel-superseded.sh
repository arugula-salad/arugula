#!/usr/bin/env bash
# scripts/queue-cancel-superseded: which merge_group runs it cancels, against
# a fake gh that answers from files written here and records what it was
# asked to cancel.
#
#   scripts/tests/queue-cancel-superseded.sh
#
# The runs still going, by pull request:
#   #10  an old group (A1) and the live one (A2): A1's runs cancelled.
#   #11  the live group stacked on #10's (its base is A2): kept.
#   #12  waits in the queue with no group built: its old group's run kept.
#   #13  merged as D2: an older group's run (D1) cancelled, and D2's own
#        run, still going after the merge, kept.
#   #14  left the queue without merging: kept.
#   #15  the queue names F2, but F2's runs are older than this one: kept.
#   #16  a newer group, but the old ref still points at G1: kept, after
#        looking again.
#   #17  a newer group on the same base, so the same ref, now at H2:
#        H1 cancelled.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
script=$here/../queue-cancel-superseded
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

# gh api [--paginate] [-X POST] [--silent] [-f K=V]... PATH [--jq EXPR]:
# answers from $FIX by path, through the real jq. A ref with no file is a
# 404, one whose file says 500 a server error. A cancel appends the run's id
# to $FAKE/cancelled.
mkdir "$tmp/bin" "$tmp/fix"
cat >"$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
[ "$1" = api ] || { echo "fake gh: $*" >&2; exit 1; }
shift
path='' jqx='.' method=GET
while [ $# -gt 0 ]; do
  case "$1" in
    --paginate|--silent) shift ;;
    -X) method=$2; shift 2 ;;
    -f) shift 2 ;;
    --jq) jqx=$2; shift 2 ;;
    *) path=$1; shift ;;
  esac
done
r=repos/forge/arugula
answer() { jq -r "$jqx" "$1"; }
case "$method $path" in
  "POST $r"/actions/runs/*/cancel)
    id=${path#"$r"/actions/runs/}; echo "${id%/cancel}" >>"$FAKE/cancelled" ;;
  "GET graphql") answer "$FIX/queue.json" ;;
  "GET $r"/actions/runs\?event=merge_group\&status=*)
    s=${path#*status=}; s=${s%%&*}
    if [ -e "$FIX/active-$s.json" ]; then answer "$FIX/active-$s.json"; else jq -nr '{workflow_runs: []}' | jq -r "$jqx"; fi ;;
  "GET $r"/actions/runs\?head_sha=*)
    sha=${path#*head_sha=}; sha=${sha%%&*}
    if [ -e "$FIX/sha-$sha.json" ]; then answer "$FIX/sha-$sha.json"; else jq -nr '{workflow_runs: []}' | jq -r "$jqx"; fi ;;
  "GET $r"/pulls/*) answer "$FIX/pull-${path#"$r"/pulls/}.json" ;;
  "GET $r"/git/ref/heads/*)
    f=$FIX/ref-$(echo "${path#"$r"/git/ref/heads/}" | tr / _).json
    if [ ! -e "$f" ]; then echo "gh: Not Found (HTTP 404)" >&2; exit 1; fi
    if grep -qx 500 "$f"; then echo "gh: Server Error (HTTP 500)" >&2; exit 1; fi
    answer "$f" ;;
  *) echo "fake gh: unexpected $method $path" >&2; exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/gh"
export PATH=$tmp/bin:$PATH FAKE=$tmp FIX=$tmp/fix GITHUB_REPOSITORY=forge/arugula QUEUE_CANCEL_SETTLE=0

q=gh-readonly-queue/main
run() { # ID NAME PR-BASE SHA MADE
  jq -n --argjson id "$1" --arg name "$2" --arg ref "$q/pr-$3" --arg sha "$4" --arg made "2026-11-16T$5:00Z" \
    '{id: $id, name: $name, head_branch: $ref, head_sha: $sha, created_at: $made}'
}
{
  run 1 check 10-ba5e1 a1 10:00
  run 2 check 10-ba5e2 a2 10:05
  run 3 check 11-a2 b1 10:06
  run 4 windows 12-ba5e1 c1 10:00
  run 5 check 13-ba5e0 d1 09:00
  run 7 check 14-ba5e0 e1 09:00
  run 8 check 15-ba5e3 f1 10:10
  run 9 check 16-ba5e4 g1 10:00
  run 10 check 17-ba5e5 h1 10:00
} | jq -s '{workflow_runs: .}' >"$tmp/fix/active-in_progress.json"
run 6 windows 13-d1 d2 09:30 | jq -s '{workflow_runs: .}' >"$tmp/fix/active-queued.json"

entry() { jq -n --argjson n "$1" --arg oid "$2" '{pullRequest: {number: $n}, headCommit: (if $oid == "" then null else {oid: $oid} end)}'; }
{
  entry 10 a2
  entry 11 b1
  entry 12 ''
  entry 15 f2
  entry 16 g2
  entry 17 h2
} | jq -s '{data: {repository: {mergeQueue: {entries: {nodes: .}}}}}' >"$tmp/fix/queue.json"

made() { # SHA HH:MM...
  local sha=$1; shift
  for t in "$@"; do jq -n --arg made "2026-11-16T$t:00Z" '{created_at: $made}'; done | jq -s '{workflow_runs: .}' >"$tmp/fix/sha-$sha.json"
}
made a2 10:05 10:05
made b1 10:06
made d2 09:30
made f2 10:00
made g2 10:05
made h2 10:05
echo '{"merged": true, "merge_commit_sha": "d2"}' >"$tmp/fix/pull-13.json"
echo '{"merged": false, "merge_commit_sha": "e9"}' >"$tmp/fix/pull-14.json"
# Live refs; #10's ba5e1, #13's and #14's are gone.
ref() { echo "{\"object\": {\"sha\": \"$2\"}}" >"$tmp/fix/ref-$(echo "$q/pr-$1" | tr / _).json"; }
ref 10-ba5e2 a2
ref 11-a2 b1
ref 16-ba5e4 g1
ref 17-ba5e5 h2

said() { # LINE NAME
  if grep -qF -- "$1" "$tmp/out"; then ok "$2"; else bad "$2: no \"$1\" in"$'\n'"$(cat "$tmp/out")"; fi
}
cancelled() { sort -n "$tmp/cancelled" | tr '\n' ' ' | sed 's/ $//'; }

: >"$tmp/cancelled"
if "$script" >"$tmp/out" 2>"$tmp/err"; then ok "runs"; else bad "exit $?: $(cat "$tmp/err")"; fi
if [ "$(cancelled)" = "1 5 10" ]; then
  ok "cancels #10's old group, #13's unmerged group and #17's group on a reused ref"
else
  bad "cancelled [$(cancelled)], want [1 5 10]"$'\n'"$(cat "$tmp/out")"
fi
said "Left check run 9 alone: $q/pr-16-ba5e4 still points at g1" \
  "looks again at a ref still there, and leaves its run"
said "Left windows run 4 on $q/pr-12-ba5e1 alone: #12 has no group now" \
  "a pull request waiting for a group keeps its runs"
said "Left check run 7 on $q/pr-14-ba5e0 alone: #14 has no group now" \
  "a pull request out of the queue, not merged, keeps its runs"
said "Left check run 8 on $q/pr-15-ba5e3 alone: #15's group f2 has no runs made after it" \
  "a group the queue names but made before the run keeps it"

: >"$tmp/cancelled"
if "$script" --dry-run >"$tmp/out" 2>"$tmp/err"; then ok "--dry-run runs"; else bad "--dry-run: exit $?: $(cat "$tmp/err")"; fi
if [ -z "$(cancelled)" ]; then ok "--dry-run cancels nothing"; else bad "--dry-run cancelled [$(cancelled)]"; fi
if [ "$(grep -c '^Would cancel' "$tmp/out")" = 3 ]; then ok "--dry-run names the three"; else bad "--dry-run said: $(cat "$tmp/out")"; fi

# A ref the API can't answer for stops the script rather than reading as gone.
echo 500 >"$tmp/fix/ref-$(echo "$q/pr-17-ba5e5" | tr / _).json"
: >"$tmp/cancelled"
if "$script" >"$tmp/out" 2>"$tmp/err"; then
  bad "a 500 on a ref passed"
else
  ok "a 500 on a ref fails the run"
fi
case " $(cancelled) " in *" 10 "*) bad "cancelled #17's run on a 500" ;; *) ok "and doesn't cancel that run" ;; esac

exit "$fail"
