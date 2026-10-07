#!/usr/bin/env bash
# `scripts/release bump` (`just release`, #524): it commits a version bump on
# top of the newest green commit on main, tags it, and pushes the tag with
# main, the bump merged in; check-green lets that tag through on its
# parent's ci/green tag. Against a local bare repository standing in for
# GitHub (GITHUB_SERVER_URL), so nothing reaches the network.
#
#   scripts/tests/release-bump.sh
set -euo pipefail

release=$(cd "$(dirname "$0")/.." && pwd)/release
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

export GITHUB_SERVER_URL=file://$tmp GITHUB_REPOSITORY=forge/arugula TMPDIR=$tmp
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
unset ARUGULA_RELEASE_SKIP_GREEN
forge=$GITHUB_SERVER_URL/$GITHUB_REPOSITORY.git
git init -q --bare "$tmp/forge/arugula.git"
git init -q -b main "$tmp/work"
cd "$tmp/work"

# A workspace in miniature: the daemon's crates at 0.1.0, the app at 0.1.0
# too, and serde, someone else's crate, at 0.1.0 as well (it must stay).
# And a client fixture, which a daemon release copies (#200).
mkdir -p crates/daemon crates/core crates/desktop crates/proto/fixtures docs/releases src
echo '{"a session":1}' >crates/proto/fixtures/session.jsonl
tree() { # DAEMON APP: the daemon's crates at DAEMON, the app at APP
  printf '[workspace]\nmembers = ["crates/*"]\nexclude = ["crates/desktop"]\n\n[workspace.package]\nversion = "%s"\nedition = "2024"\n' "$1" >Cargo.toml
  printf '[package]\nname = "arugulad"\nversion.workspace = true\n\n[dependencies]\narugula-core = { version = "%s", path = "../core" }\nserde = { version = "0.1.0" }\n' "$1" >crates/daemon/Cargo.toml
  printf '[package]\nname = "arugula-core"\nversion.workspace = true\n' >crates/core/Cargo.toml
  printf '[package]\nname = "arugula-desktop"\nversion = "%s"\n\n[dependencies]\narugula-core = { path = "../core" }\n' "$2" >crates/desktop/Cargo.toml
  { lock arugula-core "$1"; lock arugulad "$1"; lock serde 0.1.0 registry+https://github.com/rust-lang/crates.io-index; } >Cargo.lock
  { lock arugula-core "$1"; lock arugula-desktop "$2"; lock serde 0.1.0 registry+https://github.com/rust-lang/crates.io-index; } >crates/desktop/Cargo.lock
  printf -- '- [arugula-core %s](https://github.com/arugula-salad/arugula)\n- [arugulad %s](https://github.com/arugula-salad/arugula)\n- [serde 0.1.0](https://github.com/serde-rs/serde)\n' "$1" "$1" >THIRD_PARTY.md
  printf -- '- [arugula-core %s](https://github.com/arugula-salad/arugula)\n- [arugula-desktop %s](https://crates.io/crates/arugula-desktop)\n' "$1" "$2" >crates/desktop/THIRD_PARTY.md
}
lock() { # NAME VERSION [SOURCE]
  printf '[[package]]\nname = "%s"\nversion = "%s"\n' "$1" "$2"
  if [ -n "${3:-}" ]; then printf 'source = "%s"\n' "$3"; fi
  printf '\n'
}
tree 0.1.0 0.1.0
echo one >src/code
git add -A && git commit -qm one
echo two >src/code && git commit -qam two
two=$(git rev-parse HEAD)
echo three >src/code && git commit -qam three
three=$(git rev-parse HEAD)
echo four >src/code && git commit -qam four
four=$(git rev-parse HEAD)
for n in 0.1.1 0.1.2 app-0.2.0; do echo "notes $n" >"docs/releases/$n.md"; done
git add -A && git commit -qm notes
notes=$(git rev-parse HEAD)
# one and two passed CI; three passed and then failed a re-run; four and
# the notes haven't passed (yet).
git tag -a "ci/green/$(git rev-parse HEAD~4)" -m green HEAD~4
git tag -a "ci/green/$two" -m green "$two"
git tag -a "ci/green/$three" -m green "$three"
git tag -a "ci/revoked/$three" -m revoked "$three"
git push -q "$forge" main --tags

run() { # WANT NAME ARGS...: scripts/release ARGS exits WANT
  local want=$1 name=$2 got=0
  shift 2
  "$release" "$@" >"$tmp/out" 2>&1 || got=$?
  if [ "$got" = "$want" ]; then ok "$name"
  else bad "$name: exit $got, wanted $want: $(cat "$tmp/out")"; fi
}
has() { # NAME TEXT: the last run's output says TEXT
  if grep -qF -- "$2" "$tmp/out"; then ok "$1"; else bad "$1: no \"$2\" in: $(cat "$tmp/out")"; fi
}
remote() { git ls-remote "$forge" "$1" | cut -f1; }
at() { # REV FILE
  git fetch -q "$forge" main --tags
  git show "$1:$2"
}

# A dry run picks two (three is revoked), and changes nothing anywhere.
run 0 "dry run" bump --dry-run
has "dry run: the newest green commit" "v0.1.1: 0.1.0 -> 0.1.1 on ${two:0:12}"
has "dry run: what it would change" "Cargo.toml"
has "dry run: says so" "Dry run: nothing committed, tagged or pushed."
if [ -z "$(remote refs/tags/v0.1.1)" ] && [ "$(remote refs/heads/main)" = "$notes" ]; then ok "dry run: nothing pushed"
else bad "dry run pushed something"; fi
if [ "$(git worktree list | wc -l | tr -d ' ')" = 1 ]; then ok "dry run: its worktree is gone"
else bad "dry run left a worktree: $(git worktree list)"; fi

# Without notes on main, no release.
run 1 "minor: no notes on main" bump minor
has "minor: says which" "main has no docs/releases/0.2.0.md"
run 0 "minor: a dry run shows the rest" bump minor --dry-run
has "minor: the dry run says so too" "main has no docs/releases/0.2.0.md"
run 1 "a commit that isn't green" bump "$four" --dry-run
run 2 "an unknown flag" bump --force

# The app first: only crates/desktop moves, and main merges it.
run 0 "release app-v0.2.0" bump minor --app
tag=$(git rev-parse "app-v0.2.0^{commit}")
if [ "$(git rev-parse "$tag^")" = "$two" ]; then ok "app-v0.2.0 is a bump on two"; else bad "app-v0.2.0's parent isn't two"; fi
if [ "$(remote refs/tags/app-v0.2.0)" = "$(git rev-parse app-v0.2.0)" ]; then ok "app-v0.2.0 pushed"; else bad "app-v0.2.0 isn't on the forge"; fi
if [ "$(git diff --name-only "$two" "$tag" | tr '\n' ' ')" = "crates/desktop/Cargo.lock crates/desktop/Cargo.toml crates/desktop/THIRD_PARTY.md docs/releases/app-0.2.0.md " ]; then ok "the app's bump's files"
else bad "the app's bump changed: $(git diff --name-only "$two" "$tag")"; fi
main=$(remote refs/heads/main)
if git fetch -q "$forge" main && [ "$(git rev-parse FETCH_HEAD^1)" = "$notes" ] && [ "$(git rev-parse FETCH_HEAD^2)" = "$tag" ]; then ok "main: the bump merged into it"
else bad "main ($main) isn't notes + the bump"; fi
if at "$main" crates/desktop/Cargo.toml | grep -q '^version = "0.2.0"' && at "$main" crates/desktop/Cargo.lock | grep -A1 '"arugula-core"' | grep -q '0.1.0' &&
  at "$main" Cargo.toml | grep -q '^version = "0.1.0"'; then ok "main: the app at 0.2.0, the daemon still at 0.1.0"
else bad "main after the app's release: $(at "$main" crates/desktop/Cargo.toml)"; fi
run 0 "check-green app-v0.2.0 (on its parent)" check-green app-v0.2.0
has "check-green: says why" "only bumps versions: checking its parent"
run 1 "app-v0.2.0's commit again" bump minor --app --dry-run
has "again: says so" "is already released in app-v0.2.0"

# main bumps the daemon to 0.1.1 by hand, and that hasn't passed CI: the
# release from two is 0.1.2, and the merge's conflicts in the version lines
# resolve to it.
git fetch -q "$forge" main && git checkout -q FETCH_HEAD
tree 0.1.1 0.2.0
git commit -qam "Arugula 0.1.1"
git push -q "$forge" HEAD:main
run 0 "release v0.1.2 over main's 0.1.1" bump
has "v0.1.2: from main's version" "v0.1.2: 0.1.0 -> 0.1.2 on ${two:0:12}"
tag=$(git rev-parse "v0.1.2^{commit}")
if [ "$(git diff --name-only "$two" "$tag" | tr '\n' ' ')" = "Cargo.lock Cargo.toml THIRD_PARTY.md crates/daemon/Cargo.toml crates/desktop/Cargo.lock crates/desktop/THIRD_PARTY.md crates/proto/fixtures/releases/0.1.2/session.jsonl docs/releases/0.1.2.md " ]; then ok "the daemon's bump's files, the fixtures' copy among them"
else bad "the daemon's bump changed: $(git diff --name-only "$two" "$tag")"; fi
main=$(remote refs/heads/main)
if [ "$(at "$main" Cargo.lock | grep -c '"0.1.2"')" = 2 ] && at "$main" Cargo.toml | grep -q '^version = "0.1.2"' &&
  at "$main" crates/daemon/Cargo.toml | grep -q 'arugula-core = { version = "0.1.2"' &&
  at "$main" crates/desktop/Cargo.lock | grep -A1 '"arugula-core"' | grep -q '0.1.2' &&
  ! at "$main" THIRD_PARTY.md | grep -q 'arugula.* 0.1.1'; then ok "main: the daemon's crates at 0.1.2"
else bad "main isn't at 0.1.2: $(at "$main" Cargo.lock)"; fi
if at "$main" Cargo.lock | grep -A1 '"serde"' | grep -q '0.1.0' && at "$main" crates/daemon/Cargo.toml | grep -q 'serde = { version = "0.1.0" }' &&
  at "$main" THIRD_PARTY.md | grep -q 'serde 0.1.0' && at "$main" crates/desktop/Cargo.toml | grep -q '^version = "0.2.0"'; then ok "main: serde and the app keep their versions"
else bad "the bump moved serde or the app"; fi
if [ -z "$(git diff "$main^1" "$main" -- src)" ]; then ok "main: the merge changes no code"; else bad "the merge changed src"; fi
run 0 "check-green v0.1.2" check-green v0.1.2

# check-green looks past a commit only when it bumps versions.
git tag -a not-a-bump -m x "$four"
run 1 "check-green: code on top of a red commit" check-green not-a-bump
git tag -a notes-on-red -m x "$notes"
run 1 "check-green: notes on top of a red commit" check-green notes-on-red
has "notes-on-red: the parent's verdict" "is on ${four:0:12}, which has no ci/green tag"

exit $fail
