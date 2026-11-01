#!/usr/bin/env bash
# scripts/ci-red (#524): it opens one "main is red" issue when main has had
# no green commit for hours, comments on it once per red stretch, and closes
# it when main has a green commit again. A red commit's at-once notices are
# chant's tick's (ci.green.notify), which this doesn't test. With a fake gh
# that records what it's asked to write, against a local repository.
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
cat >"$tmp/bin/gh" <<'GH'
#!/usr/bin/env bash
case "$1 $2" in
  "issue list") cat "$FAKE/issue" ;;
  "issue view") cat "$FAKE/said" ;;
  "issue create"|"issue comment"|"issue close") printf '%s\n' "$*" >>"$FAKE/calls" ;;
  *) echo "fake gh: $*" >&2; exit 1 ;;
esac
GH
chmod +x "$tmp/bin/gh"
export PATH=$tmp/bin:$PATH FAKE=$tmp GITHUB_REPOSITORY=forge/arugula ARUGULA_RED_MAIN=main ARUGULA_RED_HOURS=6
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

# A revoked tag is the tick's ci.red now: no comment of its own, and a
# revoked commit doesn't count as green.
git tag "ci/green/$two" "$two"
git tag "ci/revoked/$two" "$two"
run "a revocation" 10
calls "a revocation: nothing from ci-red" '^$'
run "revoked isn't green" 10
if grep -q "already says red since ${two:0:12}" "$tmp/out"; then ok "revoked isn't green: still red since two"; else bad "revoked isn't green: $(cat "$tmp/out")"; fi

# three passes: green again, and the issue closes.
git tag "ci/green/$three" "$three"
four=$(commit four 11)
run "green again" 11
calls "green again: comments and closes" "^issue comment 7 .*Green again: ${three:0:12}.* issue close 7 -R forge/arugula $"

# Closed and green: nothing.
echo "7 CLOSED" >"$tmp/issue"
git tag "ci/green/$four" "$four"
run "closed and green" 60
calls "closed and green: nothing" '^$'

# The issue is closed and main is red for hours again: a new issue, with
# the mention, naming the new stretch.
five=$(commit five 61)
ARUGULA_RED_MENTION="@someone" run "red again" 70
calls "red again: a new issue" "^issue create .*red since $five.*@someone"
grep -o "red since $five" "$tmp/calls" >"$tmp/said"
echo "8 OPEN" >"$tmp/issue"

# A red stretch shorter than the hours, with an older green commit: the
# timer's issue closes, since main had a green commit after what it named.
git tag "ci/green/$five" "$five"
six=$(commit six 71)
run "red again, under the hours" 72
calls "short red after green: closes" "^issue comment 8 .*Green again: ${five:0:12}.* issue close 8 -R forge/arugula $"

# Still open while the stretch it names is too long, then a new stretch on
# the open issue gets one comment of its own.
echo "8 OPEN" >"$tmp/issue"
run "next stretch too long" 80
calls "a new stretch on the open issue: one comment" "^issue comment 8 -R forge/arugula --body .*red since $six"
echo "red since $six" >>"$tmp/said"
run "next stretch, next hour" 81
calls "next stretch, next hour: says it once" '^$'

exit $fail
