#!/usr/bin/env bash
# `scripts/release check-green` (#503): a release tag on a commit with a
# ci/green tag passes; one with no green tag, or with a ci/revoked tag too,
# fails, and ARUGULA_RELEASE_SKIP_GREEN=1 lets it through. `check-rare`
# (#305) the same for the ci/rare tag rare-nightly.yml writes. Against a local
# bare repository standing in for GitHub (GITHUB_SERVER_URL), so nothing
# reaches the network.
#
#   scripts/tests/release-green.sh
set -euo pipefail

release=$(cd "$(dirname "$0")/.." && pwd)/release
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }

export GITHUB_SERVER_URL=file://$tmp GITHUB_REPOSITORY=forge/arugula
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
unset ARUGULA_RELEASE_SKIP_GREEN
git init -q --bare "$tmp/forge/arugula.git"
git init -q "$tmp/work"
cd "$tmp/work"
git commit -q --allow-empty -m one
git tag -a v1.0.0 -m one
git commit -q --allow-empty -m two
git tag -a app-v1.0.0 -m two
git commit -q --allow-empty -m three
git tag -a v1.0.1 -m three
one=$(git rev-parse 'v1.0.0^{commit}') two=$(git rev-parse 'app-v1.0.0^{commit}')
git tag -a "ci/green/$one" -m green "$one"
git tag -a "ci/green/$two" -m green "$two"
git tag -a "ci/revoked/$two" -m revoked "$two"
# Only one passed the rare jobs. A ci/rare tag is lightweight, as the
# workflow makes it.
git tag "ci/rare/$one" "$one"
git push -q "$GITHUB_SERVER_URL/$GITHUB_REPOSITORY.git" --tags

expect() { # WANT CHECK TAG [ENV...]: check-CHECK's exit status is WANT
  local want=$1 check=$2 tag=$3 got=0
  shift 3
  env "$@" "$release" "check-$check" "$tag" >"$tmp/out" 2>&1 || got=$?
  if [ "$got" = "$want" ]; then printf 'ok    %s %s %s: %s\n' "$check" "$tag" "$*" "$(tail -1 "$tmp/out")"
  else bad "$check $tag $*: exit $got, wanted $want: $(cat "$tmp/out")"; fi
}
expect 0 green v1.0.0
expect 1 green app-v1.0.0
expect 1 green v1.0.1
expect 0 green v1.0.1 ARUGULA_RELEASE_SKIP_GREEN=1
expect 1 green v9.9.9
expect 0 rare v1.0.0
expect 1 rare v1.0.1
if grep -qF "just release --rare" "$tmp/out"; then printf 'ok    rare v1.0.1: says how\n'; else bad "rare v1.0.1 doesn't say how: $(cat "$tmp/out")"; fi
expect 1 rare app-v1.0.0
expect 0 rare v1.0.1 ARUGULA_RELEASE_SKIP_GREEN=1
expect 1 rare v9.9.9

exit $fail
