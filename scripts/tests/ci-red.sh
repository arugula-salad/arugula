#!/usr/bin/env bash
# scripts/ci-red (#524): it opens one "main is red" issue when main has had
# no green commit for hours or a green commit is revoked, comments on it
# once per new thing, and closes it when main is green again. With a fake
# gh that records what it's asked to write, against a local repository.
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

exit $fail
