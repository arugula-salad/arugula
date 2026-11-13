#!/usr/bin/env bash
# scripts/ci-velocity: the queue's group runs, time to merge and the first
# failing jobs, read from recorded API answers (scripts/tests/ci-velocity/)
# through a fake gh that answers by path and records what it was asked.
#
#   scripts/tests/ci-velocity.sh
#
# The fixtures: six merge_group runs over two pages (passed in 20 and 25
# min, failed in 15 and 40, timed out in 60, one cancelled); the failed
# runs' jobs (lint first in 102, e2e's shards in 105 and 106); four merged
# pull requests, one after --until, one never queued, and two whose first
# queue entry is 40 and 120 minutes before their merge.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
script=$here/../ci-velocity
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

# gh api [--paginate] [-X GET] PATH [-f K=V]...: answers from $FIXTURES and
# appends "PATH Q" to $FAKE/calls. Refuses a call without --paginate, and a
# window other than $WANT_CREATED / $WANT_MERGED when they're set.
mkdir "$tmp/bin"
cat >"$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
[ "$1" = api ] || { echo "fake gh: $*" >&2; exit 1; }
shift
path='' q='' paginate=0
while [ $# -gt 0 ]; do
  case "$1" in
    --paginate) paginate=1; shift ;;
    -X) shift 2 ;;
    -f) case "$2" in q=*) q=${2#q=} ;; esac; shift 2 ;;
    *) path=$1; shift ;;
  esac
done
echo "$path $q" >>"$FAKE/calls"
[ "$paginate" = 1 ] || { echo "fake gh: no --paginate: $path" >&2; exit 1; }
case "$path" in
  repos/forge/arugula/actions/workflows/check.yml/runs\?*)
    case "$path" in *event=merge_group*status=completed*) ;; *) echo "fake gh: not merge_group: $path" >&2; exit 1 ;; esac
    case "$path" in *"created=${WANT_CREATED:-}"*) ;; *) echo "fake gh: window: $path" >&2; exit 1 ;; esac
    cat "$FIXTURES"/runs-*.json ;;
  repos/forge/arugula/actions/runs/*/jobs\?*)
    id=${path#repos/forge/arugula/actions/runs/}; cat "$FIXTURES/jobs-${id%%/*}.json" ;;
  search/issues)
    case "$q" in *"repo:forge/arugula is:pr is:merged merged:${WANT_MERGED:-}"*) ;; *) echo "fake gh: search: $q" >&2; exit 1 ;; esac
    cat "$FIXTURES/search.json" ;;
  repos/forge/arugula/issues/*/timeline\?*)
    n=${path#repos/forge/arugula/issues/}; cat "$FIXTURES/timeline-${n%%/*}.json" ;;
  *) echo "fake gh: unexpected $path" >&2; exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/gh"
export PATH=$tmp/bin:$PATH FAKE=$tmp FIXTURES=$here/ci-velocity GITHUB_REPOSITORY=forge/arugula

export WANT_CREATED=2026-11-10T00:00:00Z..2026-11-12T23:59:59Z WANT_MERGED=2026-11-10..2026-11-12
: >"$tmp/calls"
if "$script" --since 2026-11-10 --until 2026-11-12 --json >"$tmp/json" 2>"$tmp/err"; then
  ok "--json over a window of days"
else
  bad "--json: exit $?: $(cat "$tmp/err")"
fi
is() { # JQ WANT NAME
  got=$(jq -c "$1" "$tmp/json" 2>&1)
  if [ "$got" = "$2" ]; then ok "$3"; else bad "$3: $1 is $got, want $2"; fi
}
is '.since + " " + .until' '"2026-11-10T00:00:00Z 2026-11-12T23:59:59Z"' "the window: a day's start to a day's end"
is '.queue | [.runs, .passed, .failed, .cancelled]' '[6,2,3,1]' "both pages' runs counted; timed out as failed"
is '.queue.failure_rate' '0.6' "failure rate over passed and failed, not cancelled"
is '.queue | [.wall_p50_min, .wall_p90_min]' '[25,60]' "wall time p50 and p90, without the cancelled run"
is '.time_to_merge | [.merged, .queued, .not_queued]' '[3,2,1]' "a PR merged after --until left out; one never queued counted apart"
is '.time_to_merge | [.p50_min, .p90_min]' '[40,120]' "time to merge from the first queue entry, not a later one"
is '.failures_by_job' '[{"job":"e2e","failures":2,"share":0.667},{"job":"lint","failures":1,"share":0.333}]' \
  "the first failing job, by when it finished, e2e's shards as one"
if grep -q 'runs/10[134]/jobs' "$tmp/calls"; then bad "read the jobs of a run that didn't fail: $(cat "$tmp/calls")"
else ok "jobs read only for the failed runs"; fi
if grep -q 'issues/4/timeline' "$tmp/calls"; then bad "read the timeline of a PR outside the window"
else ok "no timeline for a PR outside the window"; fi

: >"$tmp/calls"
if "$script" --since 2026-11-10 --until 2026-11-12 >"$tmp/md" 2>"$tmp/err"; then
  for line in \
    '## Merge queue: forge/arugula, 2026-11-10 00:00 to 2026-11-12 23:59 UTC' \
    '| Passed / failed / cancelled | 2 / 3 / 1 |' \
    '| Failure rate | 60% |' \
    '| Run wall time p50 / p90 | 25 / 60 min |' \
    '| PRs merged (through the queue) | 3 (2) |' \
    '| Time to merge p50 / p90 | 40 / 120 min |' \
    '| e2e | 2 | 66.7% |' \
    '| lint | 1 | 33.3% |'; do
    if grep -qxF -- "$line" "$tmp/md"; then ok "table: $line"; else bad "table lacks: $line"$'\n'"$(cat "$tmp/md")"; fi
  done
else
  bad "table: exit $?: $(cat "$tmp/err")"
fi

# An empty window: dashes, not a division by zero.
empty=$tmp/empty
mkdir "$empty"
echo '{"total_count":0,"workflow_runs":[]}' >"$empty/runs-1.json"
echo '{"total_count":0,"incomplete_results":false,"items":[]}' >"$empty/search.json"
if FIXTURES=$empty "$script" --since 2026-11-10 --until 2026-11-12 >"$tmp/md" 2>"$tmp/err"; then
  if grep -qxF '| Failure rate | – |' "$tmp/md" && grep -qxF 'No failed group runs.' "$tmp/md"; then ok "an empty window: dashes"
  else bad "an empty window:"$'\n'"$(cat "$tmp/md")"; fi
else
  bad "an empty window: exit $?: $(cat "$tmp/err")"
fi

# The default window is the last 7 days, to now.
unset WANT_CREATED WANT_MERGED
: >"$tmp/calls"
if FIXTURES=$empty "$script" --json >"$tmp/json" 2>"$tmp/err"; then
  days=$(jq '(.until | fromdate) - (.since | fromdate) | . / 86400 | round' "$tmp/json")
  if [ "$days" = 7 ]; then ok "7 days by default"; else bad "default window is $days days"; fi
else
  bad "default window: exit $?: $(cat "$tmp/err")"
fi
if "$script" --since yesterday >/dev/null 2>&1; then bad "took --since yesterday"; else ok "refuses a --since it can't read"; fi

exit "$fail"
