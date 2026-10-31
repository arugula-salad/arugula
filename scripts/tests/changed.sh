#!/usr/bin/env bash
# scripts/changed on main diffs against `chant ci base` (chant#4643): in a
# small repository with an origin holding main, chant/lifecycle and a ci/
# tag, a fake chant names the base. A path changed since the base runs the
# job, a push that changed only docs since it skips it, and no base (chant
# exits 1) or a run that isn't a push runs it. Nothing reaches the network.
#
#   scripts/tests/changed.sh
set -euo pipefail

src=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1

git init -q --bare "$tmp/origin.git"
w=$tmp/work
git init -q -b main "$w"
cd "$w"
mkdir -p scripts app docs
cp "$src/changed" scripts/
commit() { mkdir -p "$(dirname "$1")" && printf '%s\n' "$2" >"$1" && git add -A && git commit -qm "$1"; git rev-parse HEAD; }
base=$(commit app/a.rs one)
git remote add origin "$tmp/origin.git"
git push -q origin main
git tag "ci/green/$base" "$base"
git push -q origin "refs/tags/ci/green/$base"
# chant/lifecycle exists on origin, as the tick keeps it.
git checkout -q --orphan chant/lifecycle && git rm -rqf . && commit _ci/README "records" >/dev/null && git push -q origin chant/lifecycle && git checkout -q main
# A superseded commit that changed app/, then a docs-only tip.
commit app/a.rs two >/dev/null
tip=$(commit docs/readme.md docs)
git push -q origin main

# A fake chant: prints $FAKE_BASE for `ci base`, or exits 1 without one.
cat >"$tmp/chant" <<'EOF'
#!/usr/bin/env bash
[ "$1 $2" = "ci base" ] || { echo "fake chant: $*" >&2; exit 2; }
[ -n "${FAKE_BASE:-}" ] || exit 1
echo "$FAKE_BASE"
EOF
chmod +x "$tmp/chant"
export ARUGULA_CHANT="$tmp/chant" GITHUB_REF=refs/heads/main GITHUB_EVENT_NAME=push

# shellcheck disable=SC2329 # runs and skips call it
ask() { scripts/changed app "$tip~1" "$@" >"$tmp/out" 2>&1; }
# check WHAT CMD...: CMD succeeds.
check() {
  local what=$1
  shift
  if "$@"; then ok "$what"; else bad "$what: $(cat "$tmp/out")"; fi
}
# shellcheck disable=SC2329 # check runs them
runs() { FAKE_BASE=$1 ask "${@:2}"; }
# shellcheck disable=SC2329
skips() { ! FAKE_BASE=$1 ask "${@:2}"; }
# shellcheck disable=SC2329
says() { grep -qF -- "$1" "$tmp/out"; }
check "app/ changed since the base, below a docs-only tip: runs" runs "$base" app
check "nothing it reads changed since the base: skips" skips "$base" scripts
check "says which base it diffed against" says "since ${base:0:12}"
check "no base: runs" runs '' scripts
check "and says why" says "chant ci base found no commit"
GITHUB_EVENT_NAME=workflow_dispatch check "a manual run runs it" runs "$base" scripts
# The fetch brought the records' branch and the tag chant reads, and left the clone full.
check "fetched origin/chant/lifecycle" git show-ref -q --verify refs/remotes/origin/chant/lifecycle
check "has the ci/ tags" git show-ref -q --verify "refs/tags/ci/green/$base"
check "a full clone stays full" test "$(git rev-parse --is-shallow-repository)" = false

exit $fail
