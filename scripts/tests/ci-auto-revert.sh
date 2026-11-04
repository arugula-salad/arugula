#!/usr/bin/env bash
# scripts/ci-auto-revert: in a local repository with a fake chant printing a
# canned `ci list --json` and a fake gh recording what it's asked to write, a
# commit that's red after its retry with one clear culprit is reverted through
# a pull request, and every reason to leave it to a person leaves it.
#
#   scripts/tests/ci-auto-revert.sh
set -euo pipefail

script=$(cd "$(dirname "$0")/.." && pwd)/ci-auto-revert
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

mkdir "$tmp/bin"
cat >"$tmp/bin/gh" <<'GH'
#!/usr/bin/env bash
case "$1 $2" in
  "api repos/forge/arugula/commits/"*) echo "${FAKE_AUTHOR:-someone}" ;;
  "pr create")
    if [ -n "${FAKE_PR_REFUSED:-}" ]; then echo "HTTP 403: Resource not accessible" >&2; exit 1; fi
    printf '%s\n' "$*" >>"$FAKE/calls"
    echo "https://forge/pull/99"
    ;;
  "pr merge" | "pr comment") printf '%s\n' "$*" >>"$FAKE/calls" ;;
  *) echo "fake gh: $*" >&2; exit 1 ;;
esac
GH
chmod +x "$tmp/bin/gh"
cat >"$tmp/bin/chant" <<'CHANT'
#!/usr/bin/env bash
[ "$1 $2" = "ci list" ] || { echo "fake chant: $*" >&2; exit 1; }
[ -f "$FAKE/list.json" ] || { echo "fake chant: no forge" >&2; exit 1; }
cat "$FAKE/list.json"
CHANT
chmod +x "$tmp/bin/chant"

export PATH=$tmp/bin:$PATH FAKE=$tmp ARUGULA_RED_CHANT=$tmp/bin/chant GITHUB_REPOSITORY=forge/arugula
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
unset GITHUB_STEP_SUMMARY ARUGULA_AUTO_REVERT

git init -q --bare "$tmp/origin.git"
git init -q -b main "$tmp/work"
cd "$tmp/work"
git remote add origin "$tmp/origin.git"
h=3600 t0=1700000000
commit() { # FILE MESSAGE HOURS: a commit HOURS after t0 that changes FILE
  echo "$2" >>"$1"
  git add -A
  GIT_COMMITTER_DATE="@$((t0 + $3 * h)) +0000" GIT_AUTHOR_DATE="@$((t0 + $3 * h)) +0000" git commit -q -m "$2"
  git rev-parse HEAD
}
# record SHA JSON: SHA's _ci record on chant/lifecycle.
record() {
  local blob tree parent=()
  blob=$(printf '%s\n' "$2" | git hash-object -w --stdin)
  tree=$(printf '100644 blob %s\t%s.json\n' "$blob" "$1" | git mktree)
  tree=$(printf '040000 tree %s\t_ci\n' "$tree" | git mktree)
  if git rev-parse -q --verify refs/heads/chant/lifecycle >/dev/null; then parent=(-p refs/heads/chant/lifecycle); fi
  git update-ref refs/heads/chant/lifecycle "$(git commit-tree "${parent[@]}" -m record "$tree")"
}
# listing SHA STATE [CULPRIT]: chant's list, with SHA its newest decided commit.
listing() {
  jq -n --arg s "$1" --arg st "$2" --arg c "${3:-}" \
    '{commits: [{sha: "undecided", state: "undecided", record: null}, {sha: $s, state: $st, record: ({state: $st} + (if $c == "" then {} else {culprit: $c} end))}]}' >"$tmp/list.json"
}
retried='{"retried": [{"name": "test"}], "failure": {"phase": "test", "run": {"name": "test-2", "url": "https://forge/run/7"}}}'
run() { # NOW_HOURS: the script at t0 + NOW_HOURS, on main
  : >"$tmp/calls"
  git switch -q main 2>/dev/null || true
  git update-ref refs/remotes/origin/main "$(git rev-parse main)"
  git fetch -q origin 2>/dev/null || true
  ARUGULA_RED_NOW=$((t0 + $1 * h)) "$script" >"$tmp/out" 2>&1 || bad "exit $?: $(cat "$tmp/out")"
}
leaves() { # WHAT WHY: nothing was written, and the output says WHY
  if [ -s "$tmp/calls" ]; then bad "$1: gh was asked: $(cat "$tmp/calls")"
  elif grep -qF -- "$2" "$tmp/out"; then ok "$1"
  else bad "$1: said: $(cat "$tmp/out")"; fi
}
wrote() { # WHAT PATTERN: gh was asked for something matching PATTERN
  if grep -qE -- "$2" "$tmp/calls"; then ok "$1"; else bad "$1: gh was asked: $(cat "$tmp/calls"); $(cat "$tmp/out")"; fi
}

one=$(commit a.txt "one (#1)" 0)
git tag "ci/green/$one" "$one"
two=$(commit b.txt "two, for Arugula #55 (#2)" 1)
git push -q origin main

# Green: nothing to do.
listing "$one" green
run 2
leaves "green: leaves it" "is green"

# Red, but no retry has failed yet.
record "$two" '{"retried": []}'
listing "$two" red
run 2
leaves "red before its retry: leaves it" "hasn't failed its retry"

# Off.
record "$two" "$retried"
: >"$tmp/calls"
ARUGULA_AUTO_REVERT=off ARUGULA_RED_NOW=$((t0 + 2 * h)) "$script" >"$tmp/out" 2>&1
leaves "turned off: leaves it" "ARUGULA_AUTO_REVERT is off"

# Too old.
run 8
leaves "older than 6 hours: leaves it" "more than 6 hours old"

# The parent isn't green and nothing's named: more than one candidate.
three=$(commit c.txt "three (#3)" 2)
git push -q origin main
record "$three" "$retried"
listing "$three" red
run 3
leaves "two candidates: leaves it" "the bisect hasn't named a culprit"

# The token may not open a pull request: red, says so, and leaves no branch behind
# that would keep later runs out.
listing "$three" red "$two"
: >"$tmp/calls"
git switch -q main
code=0
FAKE_PR_REFUSED=1 ARUGULA_RED_NOW=$((t0 + 3 * h)) "$script" >"$tmp/out" 2>&1 || code=$?
if [ "$code" = 1 ] && grep -qF 'Revert it by hand' "$tmp/out"; then ok "a refused pull request: fails, and says to revert by hand"; else bad "exit $code: $(cat "$tmp/out")"; fi
if git ls-remote --exit-code --heads origin "auto-revert-${two:0:12}" >/dev/null 2>&1; then bad "it left its branch on origin"; else ok "and leaves no branch behind"; fi
git switch -q main
git branch -qD "auto-revert-${two:0:12}"

# The bisect names one: it's reverted, though newer work sits on top.
listing "$three" red "$two"
run 3
wrote "a named culprit: opens a pull request" "^pr create -R forge/arugula --head auto-revert-${two:0:12} "
wrote "and queues it" "^pr merge https://forge/pull/99 -R forge/arugula --auto --squash$"
wrote "and tells the author on its pull request" "^pr comment 2 -R forge/arugula --body @someone "
wrote "the text names the failure" "https://forge/run/7"
if grep -qF 'Arugula #55' "$tmp/calls"; then bad "the text names a native issue: merging would close it"; else ok "the text doesn't name the native issue"; fi
if git --git-dir="$tmp/origin.git" show "auto-revert-${two:0:12}:b.txt" >/dev/null 2>&1; then bad "the branch still has what it reverts"; else ok "the pushed branch takes the change out"; fi
if git --git-dir="$tmp/origin.git" log -1 --format=%B "auto-revert-${two:0:12}" | grep -qF "This reverts commit $two"; then ok "its message says what it reverts"; else bad "its message: $(git --git-dir="$tmp/origin.git" log -1 --format=%B "auto-revert-${two:0:12}")"; fi

# A second look doesn't open another.
run 3
leaves "its branch exists: leaves it" "exists already"
git --git-dir="$tmp/origin.git" branch -qD "auto-revert-${two:0:12}"

# It landed on main.
git revert --no-edit "$two" >/dev/null
reverted=$(git rev-parse HEAD)
git push -q origin main
run 3
leaves "reverted on main already: leaves it" "reverted on main already"

# A revert is never reverted.
record "$reverted" "$retried"
listing "$reverted" red "$reverted"
run 3
leaves "the culprit is a revert: leaves it" "is itself a revert"

# It doesn't apply: a later commit changed the same lines.
four=$(commit d.txt "four (#4)" 3)
five=$(commit d.txt "five (#5)" 3)
git push -q origin main
record "$five" "$retried"
listing "$five" red "$four"
run 4
leaves "doesn't revert cleanly: leaves it" "doesn't revert cleanly"
if [ "$(git rev-parse --abbrev-ref HEAD)" = main ] || [ -z "$(git status --porcelain)" ]; then ok "and leaves the checkout clean"; else bad "the checkout: $(git status --short)"; fi
git checkout -q main 2>/dev/null || true
git branch -qD "auto-revert-${four:0:12}" 2>/dev/null || true

# The commit below is green: the red commit is the culprit. A bot's goes untold.
git tag "ci/green/$five" "$five"
six=$(commit e.txt "six (#6)" 4)
git push -q origin main
record "$six" "$retried"
listing "$six" red
FAKE_AUTHOR='dependabot[bot]' run 5
wrote "the commit below is green: reverts the red one" "^pr create -R forge/arugula --head auto-revert-${six:0:12} "
if grep -qF '@dependabot' "$tmp/calls"; then bad "it mentions a bot"; else ok "a bot isn't mentioned"; fi

exit $fail
