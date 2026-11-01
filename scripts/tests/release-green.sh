#!/usr/bin/env bash
# `scripts/release check-green` (#503): a release tag on a commit with a
# ci/green tag passes; one with no green tag, or with a ci/revoked tag too,
# fails, and ARUGULA_RELEASE_SKIP_GREEN=1 lets it through. Each lane reads
# its own tags (#307): an app-v* tag ci/green-app, a server check
# ci/green-server; `last-green LANE` names a lane's newest green commit. `check-rare`
# (#305) the same for the ci/rare tag rare-nightly.yml writes, and
# `cut-commit` (#1986) the commit chant cuts a tag's release from. Against a local
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
git commit -q --allow-empty -m four
git tag -a app-v1.0.1 -m four
one=$(git rev-parse 'v1.0.0^{commit}') two=$(git rev-parse 'app-v1.0.0^{commit}')
three=$(git rev-parse 'v1.0.1^{commit}') four=$(git rev-parse 'app-v1.0.1^{commit}')
git tag -a "ci/green/$one" -m green "$one"
git tag -a "ci/green/$two" -m green "$two"
git tag -a "ci/revoked/$two" -m revoked "$two"
# The lanes (#307): two failed a re-run of a phase the app's lane doesn't
# need, so it's still app-green; four was app-green and then failed one
# that it does. Only three is server-green.
git tag -a "ci/green-app/$two" -m green "$two"
git tag -a "ci/green-app/$four" -m green "$four"
git tag -a "ci/revoked-app/$four" -m revoked "$four"
git tag -a "ci/green-server/$three" -m green "$three"
# Only one passed the rare jobs. A ci/rare tag is lightweight, as the
# workflow makes it.
git tag "ci/rare/$one" "$one"
git push -q "$GITHUB_SERVER_URL/$GITHUB_REPOSITORY.git" HEAD:refs/heads/main --tags

expect() { # WANT CHECK TAG [ARG|ENV...]: check-CHECK's exit status is WANT
  local want=$1 check=$2 tag=$3 got=0 args=() envs=()
  shift 3
  for a in "$@"; do case "$a" in *=*) envs+=("$a") ;; *) args+=("$a") ;; esac; done
  env ${envs[@]+"${envs[@]}"} "$release" "check-$check" "$tag" ${args[@]+"${args[@]}"} >"$tmp/out" 2>&1 || got=$?
  if [ "$got" = "$want" ]; then printf 'ok    %s %s %s: %s\n' "$check" "$tag" "$*" "$(tail -1 "$tmp/out")"
  else bad "$check $tag $*: exit $got, wanted $want: $(cat "$tmp/out")"; fi
}
expect 0 green v1.0.0
expect 1 green v1.0.0 app
expect 1 green v1.0.0 server
# An app tag reads ci/green-app: two's ci/revoked doesn't count for it.
expect 0 green app-v1.0.0
if grep -qF "(ci/green-app/$two)" "$tmp/out"; then printf 'ok    green app-v1.0.0: names the app'"'"'s tag\n'; else bad "green app-v1.0.0: $(cat "$tmp/out")"; fi
expect 1 green app-v1.0.0 daemon
expect 1 green app-v1.0.1
if grep -qF "ci/revoked-app/$four" "$tmp/out"; then printf 'ok    green app-v1.0.1: says it was revoked\n'; else bad "green app-v1.0.1: $(cat "$tmp/out")"; fi
expect 1 green v1.0.1
expect 0 green v1.0.1 server
expect 2 green v1.0.1 nightly
expect 0 green v1.0.1 ARUGULA_RELEASE_SKIP_GREEN=1
expect 1 green v9.9.9
expect 0 rare v1.0.0
expect 1 rare v1.0.1
if grep -qF "just release --rare" "$tmp/out"; then printf 'ok    rare v1.0.1: says how\n'; else bad "rare v1.0.1 doesn't say how: $(cat "$tmp/out")"; fi
expect 1 rare app-v1.0.0
expect 0 rare v1.0.1 ARUGULA_RELEASE_SKIP_GREEN=1
expect 1 rare v9.9.9

last() { # WANT LANE: `last-green LANE` prints WANT ("" for a failure)
  local got
  got=$("$release" last-green ${2:+"$2"} 2>"$tmp/out") || got=''
  if [ "$got" = "$1" ]; then printf 'ok    last-green %s: %s\n' "${2:-(daemon)}" "${got:0:12}"
  else bad "last-green ${2:-}: got '$got', wanted '$1': $(cat "$tmp/out")"; fi
}
last "$one" ''
last "$one" daemon
last "$two" app
last "$three" server
last '' nightly

cut() { # WANT TAG: `cut-commit TAG` prints WANT, the commit chant cuts from (#1986)
  local got
  got=$("$release" cut-commit "$2" 2>"$tmp/out") || got=''
  if [ "$got" = "$1" ]; then printf 'ok    cut-commit %s: %s\n' "$2" "${got:0:12}"
  else bad "cut-commit $2: got '$got', wanted '$1': $(cat "$tmp/out")"; fi
}
# A green commit is its own; one that isn't, and isn't a bump, is too, for
# chant's cut to refuse.
cut "$one" v1.0.0
cut "$three" v1.0.1
cut '' v9.9.9

exit $fail
