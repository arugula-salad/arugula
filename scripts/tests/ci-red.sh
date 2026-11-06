#!/usr/bin/env bash
# scripts/ci-red (#524): it opens one "main is red" issue when main has had
# no green commit for hours or a green commit is revoked, comments on it
# once per new thing, and closes it when main is green again. It also opens
# or comments at once when chant's record says the newest decided commit is
# red after its retry, and again when the bisect names the culprit (#1678).
# With a fake gh that records what it's asked to write and a fake chant that
# prints a canned `ci list --json`, against a local repository.
#
#   scripts/tests/ci-red.sh
set -euo pipefail

ci_red=$(cd "$(dirname "$0")/.." && pwd)/ci-red
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

# gh: `issue list` prints $tmp/issue ("NUMBER STATE", or nothing), `issue
# view` prints $tmp/said; create, comment and close append to $tmp/calls.
mkdir "$tmp/bin"
cat >"$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
case "$1 $2" in
  "issue list") cat "$FAKE/issue" ;;
  "issue view") cat "$FAKE/said" ;;
  "issue create"|"issue comment"|"issue close") printf '%s\n' "$*" >>"$FAKE/calls" ;;
  *) echo "fake gh: $*" >&2; exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/gh"
# chant: `ci list` prints $tmp/list.json, or fails when there is none.
cat >"$tmp/bin/chant" <<'EOF'
#!/usr/bin/env bash
[ "$1 $2" = "ci list" ] || { echo "fake chant: $*" >&2; exit 1; }
[ -f "$FAKE/list.json" ] || { echo "fake chant: no forge" >&2; exit 1; }
cat "$FAKE/list.json"
EOF
chmod +x "$tmp/bin/chant"
export PATH=$tmp/bin:$PATH FAKE=$tmp ARUGULA_RED_CHANT=$tmp/bin/chant GITHUB_REPOSITORY=forge/arugula ARUGULA_RED_MAIN=main ARUGULA_RED_HOURS=6
unset ARUGULA_RED_MENTION
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
git init -q -b main "$tmp/work"
cd "$tmp/work"
h=3600 t0=1700000000
commit() { # MESSAGE HOURS: a commit HOURS after t0
  GIT_COMMITTER_DATE="@$((t0 + $2 * h)) +0000" GIT_AUTHOR_DATE="@$((t0 + $2 * h)) +0000" git commit -q --allow-empty -m "$1"
  git rev-parse HEAD
}
one=$(commit one 0)
git tag "ci/green/$one" "$one"
two=$(commit two 1)
three=$(commit three 2)

: >"$tmp/issue"
: >"$tmp/said"
run() { # NAME HOURS: ci-red at t0 + HOURS, and what it wrote
  : >"$tmp/calls"
  if ARUGULA_RED_NOW=$((t0 + $2 * h)) "$ci_red" >"$tmp/out" 2>&1; then :; else bad "$1: exit $?: $(cat "$tmp/out")"; fi
}
calls() { # NAME WANT: the gh writes were WANT (a regex over them, one line)
  local got
  got=$(tr '\n' ' ' <"$tmp/calls")
  if [[ "$got" =~ $2 ]]; then ok "$1"; else bad "$1: gh was asked: ${got:-nothing}; $(cat "$tmp/out")"; fi
}

run "red for 5 hours" 5
calls "red for 5 hours: nothing yet" '^$'
run "red for 7 hours" 8
calls "red for 7 hours: opens the issue" "^issue create -R forge/arugula --title main is red --body .*red since $two"
grep -o "red since $two" "$tmp/calls" >"$tmp/said"
echo "7 OPEN" >"$tmp/issue"
run "still red" 9
calls "still red: says it once" '^$'

# two passes and then fails a re-run; three passes.
git tag "ci/green/$two" "$two"
git tag "ci/revoked/$two" "$two"
git tag "ci/green/$three" "$three"
run "a revocation" 10
calls "a revocation: comments on the open issue" "^issue comment 7 -R forge/arugula --body .*ci/revoked/$two"
echo "ci/revoked/$two" >>"$tmp/said"
four=$(commit four 11)
run "green, past the revocation" 11
calls "green again: comments and closes" "^issue comment 7 .*Green again: ${three:0:12}.* issue close 7 -R forge/arugula $"

# Closed, and a revocation older than two days isn't news.
echo "7 CLOSED" >"$tmp/issue"
git tag "ci/green/$four" "$four"
run "old revocation" 60
calls "old revocation: nothing" '^$'

# The issue is closed and something new is red: a new issue, with the
# mention.
five=$(commit five 61)
ARUGULA_RED_MENTION="@someone" run "red again" 70
calls "red again: a new issue" "^issue create .*red since $five.*@someone"

# chant's records (#1678). The newest decided commit goes red after its one
# retry: the issue opens at once, though main has been red for 1 hour.
git tag "ci/green/$five" "$five"
six=$(commit six 100)
record() { # SHA JSON: SHA's _ci record on chant/lifecycle
  local blob tree parent=()
  blob=$(printf '%s\n' "$2" | git hash-object -w --stdin)
  tree=$(printf '100644 blob %s\t%s.json\n' "$blob" "$1" | git mktree)
  tree=$(printf '040000 tree %s\t_ci\n' "$tree" | git mktree)
  if git rev-parse -q --verify refs/heads/chant/lifecycle >/dev/null; then parent=(-p refs/heads/chant/lifecycle); fi
  git update-ref refs/heads/chant/lifecycle "$(git commit-tree "${parent[@]}" -m record "$tree")"
}
list() { # RECORD: ci list with six (undecided, with RECORD) over five (green)
  cat >"$tmp/list.json" <<JSON
{"commits":[
 {"sha":"$six","state":"undecided","green":null,"revoked":null,"record":$1},
 {"sha":"$five","state":"green","green":{"tag":"ci/green/$five"},"revoked":null,"record":null}]}
JSON
}
record "$six" '{"state":"red","retried":{"nextest":{"run":9,"at":"x"}},"failure":{"phase":"test","run":{"name":"nextest","id":9,"url":"https://example.invalid/run/9"}}}'
list '{"state":"red"}'
echo "7 CLOSED" >"$tmp/issue"
: >"$tmp/said"
run "red after retry" 101
calls "red after retry: opens the issue at once" "^issue create .*red after one retry: $six\. Failed phase: test, job \"nextest\", run https://example.invalid/run/9"
grep -o "red after one retry: $six" "$tmp/calls" >"$tmp/said"
echo "8 OPEN" >"$tmp/issue"
run "red after retry, next hour" 102
calls "red after retry, next hour: says it once" '^$'
if grep -q "stays open: no green commit after" "$tmp/out"; then ok "the issue stays open while green is older than the red commit"; else bad "should stay open: $(cat "$tmp/out")"; fi

list '{"state":"red","culprit":"'"$six"'"}'
run "culprit" 103
calls "culprit: one comment naming it" "^issue comment 8 -R forge/arugula --body .*culprit: $six"
echo "culprit: $six" >>"$tmp/said"
run "culprit, next hour" 104
calls "culprit, next hour: says it once" '^$'

# The culprit of a commit the issue doesn't name isn't news.
cat >"$tmp/list.json" <<JSON
{"commits":[
 {"sha":"$six","state":"undecided","green":null,"revoked":null,"record":{"state":"red"}},
 {"sha":"$five","state":"green","green":{"tag":"ci/green/$five"},"revoked":null,"record":{"state":"red","culprit":"$five"}}]}
JSON
run "unnamed culprit" 104
calls "unnamed culprit: nothing" '^$'
printf 'red after one retry: %s\nculprit: %s\n' "$six" "$six" >"$tmp/said"
list '{"state":"red","culprit":"'"$six"'"}'

# A green commit past the red one closes the issue.
seven=$(commit seven 105)
git tag "ci/green/$seven" "$seven"
run "green past the red commit" 105
calls "green past the red commit: comments and closes" "^issue comment 8 .*Green again: ${seven:0:12}.* issue close 8 -R forge/arugula $"

# Not red after a retry: no retry recorded, or the newest decided is green.
echo "8 CLOSED" >"$tmp/issue"
: >"$tmp/said"
eight=$(commit eight 106)
record "$eight" '{"state":"red","failure":{"phase":"test","run":{"name":"nextest"}}}'
cat >"$tmp/list.json" <<JSON
{"commits":[
 {"sha":"$eight","state":"undecided","green":null,"revoked":null,"record":{"state":"red"}},
 {"sha":"$seven","state":"green","green":{"tag":"ci/green/$seven"},"revoked":null,"record":null}]}
JSON
run "red, not retried" 107
calls "red before its retry: nothing yet" '^$'
cat >"$tmp/list.json" <<JSON
{"commits":[
 {"sha":"$eight","state":"undecided","green":null,"revoked":null,"record":{"state":"pending"}},
 {"sha":"$seven","state":"green","green":{"tag":"ci/green/$seven"},"revoked":null,"record":null}]}
JSON
run "pending" 107
calls "newest decided is green: nothing" '^$'

# The chant read fails: logged, and the tags' rules still apply.
mv "$tmp/list.json" "$tmp/list.off"
run "chant fails" 108
calls "chant fails: nothing from the records" '^$'
if grep -q "chant's records not read" "$tmp/out"; then ok "chant fails: logged"; else bad "chant fails: not logged: $(cat "$tmp/out")"; fi
run "chant fails, red for 8 hours" 114
calls "chant fails: the 6 hour rule still opens the issue" "^issue create .*red since $eight"

exit $fail
