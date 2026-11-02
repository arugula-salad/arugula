#!/usr/bin/env bash
# `scripts/release bump` (`just release`, #524, #1986): it tags the newest
# green commit on main as it is, with the lane's next version after its
# newest tag, and pushes only the tag: nothing is committed, and main
# doesn't move. The commit must have the release's notes and, for the
# daemon, its client fixtures. An app release (--app) reads the app's lane,
# ci/green-app (#307). A daemon release also needs the commit's ci/rare
# tag (#305), and --rare starts rare-nightly.yml on it and waits. Then
# `stamp`, which writes a tag's version into a checkout for the release's
# build, and `check-version`. Against a local bare repository standing in
# for GitHub (GITHUB_SERVER_URL) and a fake gh, so nothing reaches the
# network.
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

# gh, for --rare: `workflow run` records its arguments in $tmp/gh-calls and
# starts a run ($tmp/gh-running, "ID URL", which `run list` prints); `run
# watch` ends it, and when FAKE_RARE is success, the run tags the commit it
# was started on ci/rare/<sha> on the forge, as rare-nightly.yml's last job
# does.
mkdir "$tmp/bin"
cat >"$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
case "$1 $2" in
  "run list") cat "$FAKE/gh-running" 2>/dev/null || true ;;
  "workflow run")
    printf '%s\n' "$*" >>"$FAKE/gh-calls"
    echo "77 https://github.com/forge/arugula/actions/runs/77" >"$FAKE/gh-running" ;;
  "run watch")
    sha=$(tail -1 "$FAKE/gh-calls" | sed -n 's/.*-f sha=\([0-9a-f]*\).*/\1/p')
    : >"$FAKE/gh-running"
    if [ "${FAKE_RARE:-}" = success ]; then git push -q "$FORGE" "$sha:refs/tags/ci/rare/$sha"; exit 0; fi
    exit 1 ;;
  *) echo "fake gh: $*" >&2; exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/gh"
export PATH=$tmp/bin:$PATH FAKE=$tmp FORGE=$forge ARUGULA_RELEASE_POLL=0
git init -q -b main "$tmp/work"
cd "$tmp/work"

# A workspace in miniature: the daemon's crates at 0.1.0, the app at 0.1.0
# too, and serde, someone else's crate, at 0.1.0 as well (it must stay).
# The server's workspace (server/) with a crate only its lock file names,
# arugula-control, at the daemon's version. And a client fixture, which a
# daemon release's commit has a copy of (#200). No commit moves a version.
mkdir -p crates/daemon crates/core crates/control crates/desktop crates/proto/fixtures docs/releases server src
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
  printf '[workspace]\nmembers = ["../crates/control"]\n\n[workspace.package]\nversion = "%s"\nedition = "2024"\n' "$1" >server/Cargo.toml
  printf '[package]\nname = "arugula-control"\nworkspace = "../../server"\nversion.workspace = true\n' >crates/control/Cargo.toml
  { lock arugula-control "$1"; lock arugula-core "$1"; lock serde 0.1.0 registry+https://github.com/rust-lang/crates.io-index; } >server/Cargo.lock
  printf -- '- [arugula-control %s](https://github.com/arugula-salad/arugula)\n- [serde 0.1.0](https://github.com/serde-rs/serde)\n' "$1" >server/THIRD_PARTY.md
}
lock() { # NAME VERSION [SOURCE]
  printf '[[package]]\nname = "%s"\nversion = "%s"\n' "$1" "$2"
  if [ -n "${3:-}" ]; then printf 'source = "%s"\n' "$3"; fi
  printf '\n'
}
tree 0.1.0 0.1.0
# NOTES N [HEADLINE]: the release's notes (a daemon release's with a
# headline section, the web client's What's new); FIXTURES N: its client
# fixtures' copy.
# The backticks are the notes' fence, not a command (SC2016).
# shellcheck disable=SC2016
notes() { if [ -n "${2:-}" ]; then printf 'notes %s\n\n```whats-new\n[]\n```\n' "$1"; else echo "notes $1"; fi >"docs/releases/$1.md"; }
fixtures() { mkdir -p "crates/proto/fixtures/releases/$1" && cp crates/proto/fixtures/session.jsonl "crates/proto/fixtures/releases/$1/"; }
echo one >src/code
git add -A && git commit -qm one
one=$(git rev-parse HEAD)
# two: what 0.1.1 and the app's 0.2.0 need.
echo two >src/code && notes 0.1.1 headline && notes app-0.2.0 && fixtures 0.1.1
git add -A && git commit -qm two
two=$(git rev-parse HEAD)
echo three >src/code && git commit -qam three
three=$(git rev-parse HEAD)
# four: what 0.1.2 needs.
echo four >src/code && notes 0.1.2 headline && fixtures 0.1.2
git add -A && git commit -qm four
four=$(git rev-parse HEAD)
# five: 0.1.3's notes, without a headline section.
echo five >src/code && notes 0.1.3 && fixtures 0.1.3
git add -A && git commit -qm five
five=$(git rev-parse HEAD)
# one and two passed CI; three passed and then failed a re-run; four and
# five haven't passed (yet).
git tag -a "ci/green/$one" -m green "$one"
git tag -a "ci/green/$two" -m green "$two"
git tag -a "ci/green/$three" -m green "$three"
git tag -a "ci/revoked/$three" -m revoked "$three"
# The app's lane (#307): one, two and three passed its phases; three's
# re-run failed a phase the app doesn't need, so it stays app-green.
for c in "$one" "$two" "$three"; do git tag -a "ci/green-app/$c" -m green "$c"; done
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
untouched() { # NAME: main is still five, and nothing in the checkout changed
  if [ "$(remote refs/heads/main)" = "$five" ] && [ -z "$(git status --porcelain)" ]; then ok "$1: main and the checkout as they were"
  else bad "$1: main is $(remote refs/heads/main), or the checkout changed: $(git status --porcelain)"; fi
}

# A dry run picks two (three is revoked), and changes nothing anywhere.
run 0 "dry run" bump --dry-run
has "dry run: the newest green commit" "v0.1.1: 0.1.0 -> 0.1.1 on ${two:0:12}"
has "dry run: says so" "Dry run: would tag ${two:0:12} v0.1.1 and push the tag; nothing tagged or pushed."
has "dry run: the merges it would leave out" "  ${three:0:7} "
has "dry run: the rare jobs haven't passed on it" "${two:0:12} has no ci/rare tag"
if [ -z "$(remote refs/tags/v0.1.1)" ] && ! git rev-parse -q --verify v0.1.1 >/dev/null; then ok "dry run: nothing tagged"
else bad "dry run tagged something"; fi
untouched "dry run"

# main's newer commits aren't green: it lists them and stops (#1679),
# unless --older says to release without them.
run 1 "newer commits not green: stops" bump
has "newer commits not green: lists them" "  ${four:0:7} "
has "newer commits not green: says how" "just release --older"
if [ -z "$(remote refs/tags/v0.1.1)" ]; then ok "newer commits not green: nothing tagged"; else bad "it tagged v0.1.1 anyway"; fi
run 2 "--older with a COMMIT" bump --older "$two"

# Without notes and fixtures in the commit, no release.
run 1 "minor: no notes in the commit" bump minor --older
has "minor: says which notes" "${two:0:12} has no docs/releases/0.2.0.md"
has "minor: and which fixtures" "just release-fixtures 0.2.0"
run 0 "minor: a dry run shows the rest" bump minor --dry-run
has "minor: the dry run says so too" "${two:0:12} has no docs/releases/0.2.0.md"
run 1 "a commit that isn't green" bump "$four" --dry-run
run 2 "an unknown flag" bump --force

# The app first. It reads ci/green-app, so it's on three, which the
# daemon's lane revoked, and the tag is on three itself.
run 0 "release app-v0.2.0" bump minor --app --older
has "app-v0.2.0: the newest app-green commit" "app-v0.2.0: 0.1.0 -> 0.2.0 on ${three:0:12}"
has "app-v0.2.0: the line the deploy driver reads" "pushed app-v0.2.0 (${three:0:12})"
if [ "$(git rev-parse "app-v0.2.0^{commit}")" = "$three" ]; then ok "app-v0.2.0 is on three itself"; else bad "app-v0.2.0 isn't on three"; fi
if [ "$(remote refs/tags/app-v0.2.0)" = "$(git rev-parse app-v0.2.0)" ]; then ok "app-v0.2.0 pushed"; else bad "app-v0.2.0 isn't on the forge"; fi
untouched "app-v0.2.0"
run 0 "check-green app-v0.2.0" check-green app-v0.2.0
has "check-green: the app's tag" "(ci/green-app/$three)"
run 1 "check-green app-v0.2.0 as the daemon's: three is revoked there" check-green app-v0.2.0 daemon
run 1 "app-v0.2.0's commit again" bump minor --app --dry-run
has "again: says so" "is already released in app-v0.2.0"

ARUGULA_RELEASE_TRAILER=$'two\nlines' run 2 "a trailer of two lines" bump --dry-run
ARUGULA_RELEASE_TRAILER="no colon" run 2 "a trailer that isn't Key: value" bump --dry-run
# two hasn't passed the rare jobs (#305): no daemon release, and --rare
# starts them on it and waits; a run that fails releases nothing.
run 1 "no ci/rare tag: stops" bump --older
has "no ci/rare tag: says how" "just release --rare"
if [ -z "$(remote refs/tags/v0.1.1)" ]; then ok "no ci/rare tag: nothing pushed"; else bad "it pushed v0.1.1 without the rare jobs"; fi
FAKE_RARE=failure run 1 "--rare: the run fails" bump --older --rare
has "--rare: the run it started" "actions/runs/77"
if grep -qx "workflow run rare-nightly.yml -R forge/arugula --ref main -f sha=$two" "$tmp/gh-calls"; then ok "--rare: started rare-nightly.yml on two"
else bad "--rare started: $(cat "$tmp/gh-calls" 2>/dev/null)"; fi
if [ -z "$(remote refs/tags/v0.1.1)" ]; then ok "--rare, failed: nothing pushed"; else bad "it pushed v0.1.1 after the rare jobs failed"; fi
# As the arugula-release deploy driver runs it (M221): its trailer on the
# tag. (The driver never passes --rare; here it runs the rare jobs first,
# which pass.)
ARUGULA_RELEASE_TRAILER="Arugula-Deploy: dep_m221" FAKE_RARE=success run 0 "release v0.1.1" bump --older --rare
has "v0.1.1: after the rare jobs passed" "${two:0:12} passed the rare jobs (ci/rare/$two)"
has "v0.1.1: the line the deploy driver reads" "pushed v0.1.1 (${two:0:12})"
if [ "$(git rev-parse "v0.1.1^{commit}")" = "$two" ] && [ "$(remote 'refs/tags/v0.1.1^{}')" = "$two" ]; then ok "v0.1.1 is on two itself, on the forge"
else bad "v0.1.1 isn't on two: $(git rev-parse "v0.1.1^{commit}")"; fi
if git cat-file -p v0.1.1 | grep -qx "Arugula-Deploy: dep_m221"; then ok "v0.1.1: the deploy's trailer on the tag"
else bad "v0.1.1: no trailer: $(git cat-file -p v0.1.1)"; fi
untouched "v0.1.1"
run 0 "check-green v0.1.1" check-green v0.1.1
run 0 "check-rare v0.1.1" check-rare v0.1.1
run 0 "check-version v0.1.1: its fixtures and the rare jobs" check-version v0.1.1

# The next daemon release counts from the newest v* tag, since the files
# still say 0.1.0: four, once green and past the rare jobs, is 0.1.2.
git tag -a "ci/green/$four" -m green "$four" && git tag "ci/rare/$four" "$four" && git push -q "$forge" --tags
run 0 "release v0.1.2 from the newest tag" bump "$four"
has "v0.1.2: from v0.1.1, not the files' 0.1.0" "v0.1.2: 0.1.1 -> 0.1.2 on ${four:0:12}"
if [ "$(git rev-parse "v0.1.2^{commit}")" = "$four" ]; then ok "v0.1.2 is on four"; else bad "v0.1.2 isn't on four"; fi
untouched "v0.1.2"

# Notes without a headline section: no release, but a dry run goes on.
git tag -a "ci/green/$five" -m green "$five" && git tag "ci/rare/$five" "$five" && git push -q "$forge" --tags
run 1 "v0.1.3: no headline section" bump "$five"
has "v0.1.3: says so" "docs/releases/0.1.3.md has no headline section"
if [ -z "$(remote refs/tags/v0.1.3)" ]; then ok "v0.1.3: nothing pushed"; else bad "v0.1.3 was pushed"; fi
run 0 "v0.1.3: a dry run shows the rest" bump "$five" --dry-run
has "v0.1.3: the dry run says so too" "docs/releases/0.1.3.md has no headline section"

# check-version: a tag on a commit without the version's fixtures.
git tag -a v0.1.9 -m x "$three"
run 1 "check-version v0.1.9: no fixtures for it" check-version v0.1.9
has "check-version v0.1.9: says how" "just release-fixtures 0.1.9"
run 2 "check-version: not a release tag" check-version vnext

# stamp: a tag's version into the checkout, as a release's build does it.
git checkout -q "$two"
run 0 "stamp v0.1.7" stamp v0.1.7
has "stamp v0.1.7: says what" "stamped v0.1.7: 0.1.0 -> 0.1.7 (daemon)"
if grep -q '^version = "0.1.7"' Cargo.toml && [ "$(grep -c '"0.1.7"' Cargo.lock)" = 2 ] &&
  grep -q 'arugula-core = { version = "0.1.7"' crates/daemon/Cargo.toml &&
  grep -A1 '"arugula-core"' crates/desktop/Cargo.lock | grep -q '0.1.7' && grep -q 'arugulad 0.1.7' THIRD_PARTY.md; then ok "stamp v0.1.7: the daemon's crates at 0.1.7"
else bad "stamp v0.1.7: $(git diff)"; fi
if grep -q '^version = "0.1.7"' server/Cargo.toml && [ "$(grep -c '"0.1.7"' server/Cargo.lock)" = 2 ] &&
  grep -q 'arugula-control 0.1.7' server/THIRD_PARTY.md; then ok "stamp v0.1.7: the server's workspace at 0.1.7 too"
else bad "stamp v0.1.7: the server's workspace: $(git diff -- server)"; fi
if grep -A1 '"serde"' Cargo.lock | grep -q '0.1.0' && grep -q 'serde = { version = "0.1.0" }' crates/daemon/Cargo.toml &&
  grep -q 'serde 0.1.0' THIRD_PARTY.md && grep -q '^version = "0.1.0"' crates/desktop/Cargo.toml; then ok "stamp v0.1.7: serde and the app keep their versions"
else bad "stamp v0.1.7 moved serde or the app"; fi
if [ "$(git diff --name-only | tr '\n' ' ')" = "Cargo.lock Cargo.toml THIRD_PARTY.md crates/daemon/Cargo.toml crates/desktop/Cargo.lock crates/desktop/THIRD_PARTY.md server/Cargo.lock server/Cargo.toml server/THIRD_PARTY.md " ]; then ok "stamp v0.1.7: only version files"
else bad "stamp v0.1.7 changed: $(git diff --name-only)"; fi
before=$(git diff)
run 0 "stamp v0.1.7 again" stamp v0.1.7
if [ "$(git diff)" = "$before" ]; then ok "stamp v0.1.7 again: nothing more"; else bad "stamp v0.1.7 again changed more"; fi
git checkout -q -- .
run 0 "stamp app-v0.3.0" stamp app-v0.3.0
if grep -q '^version = "0.3.0"' crates/desktop/Cargo.toml && grep -A1 '"arugula-desktop"' crates/desktop/Cargo.lock | grep -q '0.3.0' &&
  grep -q '^version = "0.1.0"' Cargo.toml; then ok "stamp app-v0.3.0: the app alone"
else bad "stamp app-v0.3.0: $(git diff)"; fi
git checkout -q -- .
run 2 "stamp: not a release tag" stamp v1.2
git checkout -q main

exit $fail
