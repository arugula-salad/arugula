#!/usr/bin/env bash
# `scripts/release bump` (`just release`, #524, #1986): it tags the newest
# green commit on main as it is, with the lane's next version after its
# newest tag, and pushes the tag: no commit moves the version. The notes go in
# the tag's message (#707), from --notes or from the commit, and it refuses a
# PR they leave out or that the tag doesn't have. It then commits to main the
# notes and, for the daemon, the tag's client fixtures (#200): again on a main
# that moved under it, not at all when main has them, and only a warning
# when main refuses it. An app release (--app) reads the app's lane,
# ci/green-app (#307). A daemon release also needs
# the commit's ci/rare tag (#305), and --rare starts rare-nightly.yml on it
# and waits. Then `stamp`, which writes a tag's version and notes into a
# checkout for the release's build, `draft`, which reads the notes for the
# GitHub release and leaves the checkout clean for chant's publish,
# `check-version` and `check-fixtures`. Against a
# local bare repository standing in for GitHub (GITHUB_SERVER_URL) and a
# fake gh, so nothing reaches the network.
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
# does. `run rerun` records itself in $tmp/gh-reruns, and FAKE_RERUN=success
# passes the watch after it. `run view` says rare-passed's conclusion
# (FAKE_RARE_PASSED, default skipped). For draft: every release is a draft, and `release edit` keeps its
# notes in $tmp/release-body.
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
    if [ "${FAKE_RARE:-}" = success ] || { [ -s "$FAKE/gh-reruns" ] && [ "${FAKE_RERUN:-}" = success ]; }; then
      git push -q "$FORGE" "$sha:refs/tags/ci/rare/$sha"; exit 0
    fi
    exit 1 ;;
  "run view")
    case "$*" in
      *"--json jobs"*) echo "${FAKE_RARE_PASSED:-skipped}" ;;
      *) echo in_progress ;;
    esac ;;
  "run rerun") printf '%s\n' "$*" >>"$FAKE/gh-reruns" ;;
  "release view") if [ "${6:-}" = --json ]; then echo true; fi ;;
  "release edit") while [ "$#" != 0 ]; do if [ "$1" = --notes-file ]; then cp "$2" "$FAKE/release-body"; fi; shift; done ;;
  *) echo "fake gh: $*" >&2; exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/gh"
export PATH=$tmp/bin:$PATH FAKE=$tmp FORGE=$forge ARUGULA_RELEASE_POLL=0
git init -q -b main "$tmp/work"
cd "$tmp/work"

# A workspace in miniature: the daemon's crates at 0.1.0, the app at 0.1.0
# too, and serde, someone else's crate, at 0.1.0 as well (it must stay).
# And a client fixture, which
# changes after two, so a release of two must copy two's (#200). No commit
# moves a version.
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
# NOTES N [HEADLINE]: the release's notes (a daemon release's with a
# headline section, the web client's What's new); FIXTURES N: its client
# fixtures' copy, as `just release-fixtures` makes it.
# The backticks are the notes' fence, not a command (SC2016).
# shellcheck disable=SC2016
notes() { if [ -n "${2:-}" ]; then printf 'notes %s\n\n```whats-new\n[]\n```\n' "$1"; else echo "notes $1"; fi >"docs/releases/$1.md"; }
fixtures() { mkdir -p "crates/proto/fixtures/releases/$1" && cp crates/proto/fixtures/session.jsonl "crates/proto/fixtures/releases/$1/"; }
echo one >src/code
git add -A && git commit -qm one
one=$(git rev-parse HEAD)
# two: what 0.1.1 and the app's 0.2.0 need: their notes, and no copy of
# the fixtures.
echo two >src/code && notes 0.1.1 headline && notes app-0.2.0
git add -A && git commit -qm two
two=$(git rev-parse HEAD)
echo three >src/code && echo '{"a session":3}' >crates/proto/fixtures/session.jsonl && git commit -qam three
three=$(git rev-parse HEAD)
# four: what 0.1.2 needs, and its fixtures' copy made by hand already.
echo four >src/code && notes 0.1.2 headline && fixtures 0.1.2
git add -A && git commit -qm four
four=$(git rev-parse HEAD)
# five: 0.1.3's notes, without a headline section.
echo five >src/code && notes 0.1.3
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
untouched() { # NAME [MAIN]: main is still MAIN (five), and nothing in the checkout changed
  if [ "$(remote refs/heads/main)" = "${2:-$five}" ] && [ -z "$(git status --porcelain)" ] && [ "$(git rev-parse HEAD)" = "$five" ]; then ok "$1: main and the checkout as they were"
  else bad "$1: main is $(remote refs/heads/main), or the checkout changed: $(git status --porcelain)"; fi
}
# The forge's main refuses pushes while $tmp/refuse-main exists; with
# $tmp/move-main, the next push to main finds that main moved (someone
# merged a commit, "moved", just before) and fails, as GitHub's would.
hook=$tmp/forge/arugula.git/hooks/pre-receive
cat >"$hook" <<'HOOK'
#!/usr/bin/env bash
while read -r old new ref; do
  [ "$ref" = refs/heads/main ] || continue
  if [ -e "$FAKE/refuse-main" ]; then echo "main: protected" >&2; exit 1; fi
  if [ -e "$FAKE/move-main" ]; then
    rm "$FAKE/move-main"
    q() { env -u GIT_QUARANTINE_PATH -u GIT_OBJECT_DIRECTORY -u GIT_ALTERNATE_OBJECT_DIRECTORIES git "$@"; }
    moved=$(q commit-tree "$old^{tree}" -p "$old" -m moved)
    q update-ref refs/heads/main "$moved" "$old"
  fi
done
HOOK
chmod +x "$hook"

# A dry run picks two (three is revoked), and changes nothing anywhere.
run 0 "dry run" bump --dry-run
has "dry run: the newest green commit" "v0.1.1: 0.1.0 -> 0.1.1 on ${two:0:12}"
has "dry run: says so" "Dry run: would tag ${two:0:12} v0.1.1 with its notes, push the tag, then commit its client fixtures and notes to main (crates/proto/fixtures/releases/0.1.1, docs/releases/0.1.1.md); nothing tagged or pushed."
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

# Without notes in the commit, no release. The fixtures aren't asked for.
run 1 "minor: no notes in the commit" bump minor --older
has "minor: says which notes" "${two:0:12} has no docs/releases/0.2.0.md"
if grep -q fixtures "$tmp/out"; then bad "minor: asks for fixtures: $(cat "$tmp/out")"; else ok "minor: doesn't ask for the fixtures"; fi
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
if [ "$(grep -c 'run rerun 77 .*--failed' "$tmp/gh-reruns" 2>/dev/null)" = 1 ]; then ok "--rare: the failed jobs got one more try"
else bad "--rare reruns: $(cat "$tmp/gh-reruns" 2>/dev/null)"; fi
: >"$tmp/gh-reruns"
# A person's cancel takes rare-passed with it, and stays cancelled.
FAKE_RARE=failure FAKE_RARE_PASSED=cancelled run 1 "--rare: cancelled by hand" bump --older --rare
has "--rare: says it was cancelled" "were cancelled"
if [ -s "$tmp/gh-reruns" ]; then bad "--rare re-ran a cancelled run: $(cat "$tmp/gh-reruns")"; else ok "--rare: a cancelled run isn't run again"; fi
# As the arugula-release deploy driver runs it (M221): its trailer on the
# tag. (The driver never passes --rare; here it runs the rare jobs first,
# which pass.) Main moves just as the fixtures go to it.
touch "$tmp/move-main"
# They pass on the second try.
ARUGULA_RELEASE_TRAILER="Arugula-Deploy: dep_m221" FAKE_RARE=failure FAKE_RERUN=success run 0 "release v0.1.1" bump --older --rare
has "v0.1.1: the failed jobs ran once more" "running the failed ones once more"
has "v0.1.1: after the rare jobs passed" "${two:0:12} passed the rare jobs (ci/rare/$two)"
has "v0.1.1: the line the deploy driver reads" "pushed v0.1.1 (${two:0:12})"
if [ "$(git rev-parse "v0.1.1^{commit}")" = "$two" ] && [ "$(remote 'refs/tags/v0.1.1^{}')" = "$two" ]; then ok "v0.1.1 is on two itself, on the forge"
else bad "v0.1.1 isn't on two: $(git rev-parse "v0.1.1^{commit}")"; fi
if git cat-file -p v0.1.1 | grep -qx "Arugula-Deploy: dep_m221"; then ok "v0.1.1: the deploy's trailer on the tag"
else bad "v0.1.1: no trailer: $(git cat-file -p v0.1.1)"; fi
# The fixtures, two's and not main's, in one commit on main, on top of the
# commit that moved main; the checkout as it was.
main1=$(remote refs/heads/main)
git fetch -q "$forge" main
moved=$(git rev-parse "$main1^")
if [ "$(git log -1 --format=%s "$moved")" = moved ] && [ "$(git rev-parse "$moved^")" = "$five" ]; then ok "v0.1.1: the fixtures on the main that moved"
else bad "v0.1.1: main is $(git log --format='%h %s' -3 "$main1")"; fi
has "v0.1.1: says it committed them" "committed crates/proto/fixtures/releases/0.1.1 to main (${main1:0:12})"
if [ "$(git diff --name-only "$moved" "$main1")" = crates/proto/fixtures/releases/0.1.1/session.jsonl ] &&
  [ "$(git show "$main1:crates/proto/fixtures/releases/0.1.1/session.jsonl")" = '{"a session":1}' ]; then ok "v0.1.1: two's fixtures, nothing else"
else bad "v0.1.1: the fixtures commit: $(git show --stat "$main1")"; fi
untouched "v0.1.1" "$main1"
run 0 "check-green v0.1.1" check-green v0.1.1
run 0 "check-rare v0.1.1" check-rare v0.1.1
run 0 "check-version v0.1.1: the rare jobs" check-version v0.1.1
run 0 "check-fixtures v0.1.1" check-fixtures v0.1.1
has "check-fixtures v0.1.1: main has them" "main has crates/proto/fixtures/releases/0.1.1"

# The next daemon release counts from the newest v* tag, since the files
# still say 0.1.0: four, once green and past the rare jobs, is 0.1.2.
git tag -a "ci/green/$four" -m green "$four" && git tag "ci/rare/$four" "$four" && git push -q "$forge" --tags
run 0 "release v0.1.2 from the newest tag" bump "$four"
has "v0.1.2: from v0.1.1, not the files' 0.1.0" "v0.1.2: 0.1.1 -> 0.1.2 on ${four:0:12}"
if [ "$(git rev-parse "v0.1.2^{commit}")" = "$four" ]; then ok "v0.1.2 is on four"; else bad "v0.1.2 isn't on four"; fi
# four has its copy already, and so main has: nothing to commit.
has "v0.1.2: main has the fixtures already" "main has crates/proto/fixtures/releases/0.1.2 already, as v0.1.2 has it"
untouched "v0.1.2" "$main1"

# Notes without a headline section: no release, but a dry run goes on.
git tag -a "ci/green/$five" -m green "$five" && git tag "ci/rare/$five" "$five" && git push -q "$forge" --tags
run 1 "v0.1.3: no headline section" bump "$five"
has "v0.1.3: says so" "docs/releases/0.1.3.md has no headline section"
if [ -z "$(remote refs/tags/v0.1.3)" ]; then ok "v0.1.3: nothing pushed"; else bad "v0.1.3 was pushed"; fi
run 0 "v0.1.3: a dry run shows the rest" bump "$five" --dry-run
has "v0.1.3: the dry run says so too" "docs/releases/0.1.3.md has no headline section"

# six, 0.1.3 with its headline, on main: main refuses the fixtures
# commit. The release stands, and it says what to run.
git checkout -q -b six "$main1"
echo six >src/code && notes 0.1.3 headline
git add -A && git commit -qm six
six=$(git rev-parse HEAD)
git tag -a "ci/green/$six" -m green "$six" && git tag "ci/rare/$six" "$six"
git push -q "$forge" "$six:refs/heads/main" "refs/tags/ci/green/$six" "refs/tags/ci/rare/$six"
touch "$tmp/refuse-main"
run 0 "v0.1.3: main refuses the fixtures" bump "$six"
has "v0.1.3: pushed" "pushed v0.1.3 (${six:0:12})"
has "v0.1.3: says what to run" "warning: main refused crates/proto/fixtures/releases/0.1.3 (tried 5 times). v0.1.3 is released all the same; run \`just release-fixtures 0.1.3\` and merge it to main"
if [ "$(remote 'refs/tags/v0.1.3^{}')" = "$six" ] && [ "$(remote refs/heads/main)" = "$six" ]; then ok "v0.1.3: the tag is out, main as it was"
else bad "v0.1.3: tag $(remote 'refs/tags/v0.1.3^{}'), main $(remote refs/heads/main)"; fi
rm "$tmp/refuse-main"
# check-fixtures, as release.yml's fixtures job runs it: a warning in the
# job's summary, never a failure.
GITHUB_ACTIONS=true GITHUB_STEP_SUMMARY=$tmp/summary run 0 "check-fixtures v0.1.3: none on main" check-fixtures v0.1.3
has "check-fixtures v0.1.3: an Actions warning" "::warning::main has no crates/proto/fixtures/releases/0.1.3"
has "check-fixtures v0.1.3: main has the notes" "main has docs/releases/0.1.3.md"
# The backticks are the message's, not a command (SC2016).
# shellcheck disable=SC2016
if grep -qF 'run `just release-fixtures 0.1.3` and merge it to main' "$tmp/summary"; then ok "check-fixtures v0.1.3: in the summary"
else bad "check-fixtures v0.1.3: summary: $(cat "$tmp/summary" 2>/dev/null)"; fi
git checkout -q main

# The notes travel with the tag (#707). Main moves on past the commit a release
# is cut from: seven and eight are its PRs, a notes-only PR isn't one the notes
# answer for, and late landed after it. No commit carries the notes.
git checkout -q -b later "$six"
mkdir -p scripts && cp "$(dirname "$release")/whats-new.mjs" scripts/
echo seven >src/code && git add -A && git commit -qm "A fix (#101)"
echo eight >src/code && git commit -qam "A refactor (#102)"
echo 'one more line' >>docs/releases/0.1.3.md && git commit -qam "Notes: 0.1.3 adds a line (#103) [skip ci]"
pick=$(git rev-parse HEAD)
echo late >src/code && git commit -qam "Too late (#104)"
late=$(git rev-parse HEAD)
for c in "$pick" "$late"; do git tag -a "ci/green/$c" -m green "$c" && git tag "ci/rare/$c" "$c"; done
git push -q "$forge" "$late:refs/heads/main" "refs/tags/ci/green/$pick" "refs/tags/ci/rare/$pick" "refs/tags/ci/green/$late" "refs/tags/ci/rare/$late"
nothing_pushed() { # NAME
  if [ -z "$(remote refs/tags/v0.1.4)" ] && [ "$(remote refs/heads/main)" = "$late" ]; then ok "$1: nothing pushed"
  else bad "$1: tag $(remote refs/tags/v0.1.4), main $(remote refs/heads/main)"; fi
}
# The backticks are the notes' fences, not commands (SC2016).
# shellcheck disable=SC2016
write() { # FILE MENTIONS [NO-NOTES]: notes that mention MENTIONS, with a heading git could take for a comment
  { printf '# Arugula next\n\nThe fix (%s).\n\n```whats-new\n[{"id": "from-the-tag", "title": "From the tag", "line": "It came with the tag."}]\n```\n' "$2"
    if [ -n "${3:-}" ]; then printf '\n```no-notes\n%s\n```\n' "$3"; fi; } >"$1"
}
write "$tmp/some.md" "#101"
write "$tmp/all.md" "#101" "#102"
write "$tmp/too-many.md" "#101, #102, #104"
write "$tmp/prefix.md" "#1010, #102"
printf 'just a line, and #101 #102\n' >"$tmp/no-headline.md"
# shellcheck disable=SC2016
printf '# Arugula next, #101 #102\n\n```whats-new\n[not json\n```\n' >"$tmp/bad-json.md"
# shellcheck disable=SC2016
printf '# Arugula next, #101 #102\n\n```whats-new\n[{"id": "agent-one", "title": "x", "line": "x"}, {"id": "agent-one", "title": "y", "line": "y"}]\n```\n' >"$tmp/twice.md"

# A dry run is where a release starts: the PRs, and the command that holds the
# commit. The notes-only PR isn't listed, and nothing says it's "left out".
run 0 "notes: a dry run of the commit" bump "$pick" --dry-run
has "notes: a dry run lists a PR" "A fix (#101)"
has "notes: a dry run lists the other" "A refactor (#102)"
if grep -q '^  #103' "$tmp/out"; then bad "notes: lists the notes-only PR: $(cat "$tmp/out")"; else ok "notes: not the notes-only PR"; fi
has "notes: no notes yet" "${pick:0:12} has no docs/releases/0.1.4.md"
has "notes: says what to run, naming the commit" "To release it: just release $pick --notes FILE"
run 2 "--notes with no file" bump "$pick" --notes
run 2 "--notes with an empty file" bump "$pick" --notes /dev/null

# The refusals: a PR left out, a PR the tag doesn't have, headlines the build
# would choke on. None of them pushes anything.
run 1 "notes: a PR left out" bump "$pick" --notes "$tmp/some.md"
has "notes: names the PR left out" "$tmp/some.md leaves out #102"
has "notes: marks it in the list" "A refactor (#102)  <- not in the notes"
has "notes: says how" "no-notes block"
nothing_pushed "notes: a PR left out"
run 1 "notes: #1010 isn't #101" bump "$pick" --notes "$tmp/prefix.md"
has "notes: #1010 isn't #101: says so" "leaves out #101"
nothing_pushed "notes: #1010 isn't #101"
run 1 "notes: a PR the tag doesn't have" bump "$pick" --notes "$tmp/too-many.md"
has "notes: names the PR that landed after" "mentions #104, which landed on main after ${pick:0:12}"
nothing_pushed "notes: a PR the tag doesn't have"
run 1 "notes: no headline section" bump "$pick" --notes "$tmp/no-headline.md"
has "notes: says which file" "$tmp/no-headline.md has no headline section"
run 1 "notes: headlines that aren't JSON" bump "$pick" --notes "$tmp/bad-json.md"
has "notes: says what's wrong" "the whats-new block isn't JSON"
run 1 "notes: the same headline id twice" bump "$pick" --notes "$tmp/twice.md"
has "notes: says which id" "the headline id agent-one"
nothing_pushed "notes: refused headlines"
{ cat "$tmp/all.md"; echo '<!-- end of release notes -->'; } >"$tmp/marker.md"
run 1 "notes: the tag's own marker in them" bump "$pick" --notes "$tmp/marker.md"
has "notes: says which line" "has a line the tag's message keeps the notes between"
run 0 "notes: a dry run with notes that pass" bump "$pick" --notes "$tmp/all.md" --dry-run
if grep -qF 'not in the notes' "$tmp/out"; then bad "notes: dry run: $(cat "$tmp/out")"; else ok "notes: a dry run with notes that pass says nothing's left out"; fi
nothing_pushed "notes: a dry run with notes"

# The release: with main moving under it. The tag is on the commit CI passed,
# with no commit before it, and the notes are in its message as written.
touch "$tmp/move-main"
ARUGULA_RELEASE_TRAILER="Arugula-Deploy: dep_707" run 0 "notes: release v0.1.4" bump "$pick" --notes "$tmp/all.md"
has "notes: v0.1.4: the line the deploy driver reads" "pushed v0.1.4 (${pick:0:12})"
if [ "$(remote 'refs/tags/v0.1.4^{}')" = "$pick" ]; then ok "notes: v0.1.4 is the commit CI passed"; else bad "notes: v0.1.4 is on $(remote 'refs/tags/v0.1.4^{}')"; fi
if git cat-file -e "$pick:docs/releases/0.1.4.md" 2>/dev/null; then bad "notes: the commit has the notes"; else ok "notes: the commit has no notes file"; fi
sed '1,/^$/d' <<<"$(git cat-file tag v0.1.4)" >"$tmp/message"
if sed -n 1p "$tmp/message" | grep -qx 'arugula 0.1.4' && tail -1 "$tmp/message" | grep -qx 'Arugula-Deploy: dep_707' && grep -qx '# Arugula next' "$tmp/message"; then ok "notes: v0.1.4's message: title, notes with their heading, trailer last"
else bad "notes: v0.1.4's message: $(cat "$tmp/message")"; fi
run 0 "notes: scripts/release notes v0.1.4" notes v0.1.4
if diff -q "$tmp/out" "$tmp/all.md" >/dev/null; then ok "notes: v0.1.4's notes are the file, byte for byte"; else bad "notes: v0.1.4's notes: $(diff "$tmp/out" "$tmp/all.md")"; fi

# Then main gets the notes and the fixtures, in one commit, on the main that
# moved; check-fixtures agrees, and the fixtures are the tag's.
main2=$(remote refs/heads/main)
git fetch -q "$forge" main
moved2=$(git rev-parse "$main2^")
if [ "$(git log -1 --format=%s "$moved2")" = moved ] && [ "$(git rev-parse "$moved2^")" = "$late" ]; then ok "notes: on the main that moved"
else bad "notes: main is $(git log --format='%h %s' -3 "$main2")"; fi
if [ "$(git diff --name-only "$moved2" "$main2" | tr '\n' ' ')" = "crates/proto/fixtures/releases/0.1.4/session.jsonl docs/releases/0.1.4.md " ]; then ok "notes: the commit on main: fixtures and notes"
else bad "notes: the commit on main: $(git diff --name-only "$moved2" "$main2")"; fi
git show "$main2:docs/releases/0.1.4.md" >"$tmp/on-main"
if diff -q "$tmp/on-main" "$tmp/all.md" >/dev/null; then ok "notes: main's copy is the file"; else bad "notes: main's copy: $(cat "$tmp/on-main")"; fi
run 0 "notes: check-fixtures v0.1.4" check-fixtures v0.1.4
has "notes: check-fixtures: the fixtures" "main has crates/proto/fixtures/releases/0.1.4"
has "notes: check-fixtures: the notes" "main has docs/releases/0.1.4.md"

# A build job's checkout: the tag's commit, with the annotated tag replaced by
# a lightweight one, as actions/checkout does on a tag push. What `stamp` writes
# is what What's new's build reads (scripts/whats-new.mjs is the web build's).
git -C "$tmp/forge/arugula.git" config uploadpack.allowAnySHA1InWant true
checkout() { # DIR TAG SHA [--depth=1]: DIR as a tag push's actions/checkout leaves it
  git init -q "$1"
  if [ -z "${4:-}" ]; then git -C "$1" fetch -q "$forge" '+refs/heads/*:refs/remotes/origin/*' '+refs/tags/*:refs/tags/*'; fi
  git -C "$1" fetch -q --no-tags ${4:+"$4"} "$forge" "+$3:refs/tags/$2"
  git -C "$1" checkout -q --force "refs/tags/$2"
}
in_dir() { # DIR ARGS...: scripts/release ARGS, in DIR
  local got=0 dir=$1
  shift
  (cd "$dir" && "$release" "$@") >"$tmp/out" 2>&1 || got=$?
  return "$got"
}
checkout "$tmp/build" v0.1.4 "$pick"
if [ "$(git -C "$tmp/build" cat-file -t refs/tags/v0.1.4)" = commit ]; then ok "stamp: the checkout's tag is lightweight"; else bad "stamp: the checkout's tag is annotated"; fi
if in_dir "$tmp/build" stamp v0.1.4; then ok "stamp v0.1.4"; else bad "stamp v0.1.4: $(cat "$tmp/out")"; fi
has "stamp v0.1.4: the version" "stamped v0.1.4: 0.1.0 -> 0.1.4 (daemon)"
has "stamp v0.1.4: the notes" "stamped v0.1.4's notes into docs/releases/0.1.4.md"
if diff -q "$tmp/build/docs/releases/0.1.4.md" "$tmp/all.md" >/dev/null; then ok "stamp v0.1.4: the notes are the tag's"; else bad "stamp v0.1.4: $(cat "$tmp/build/docs/releases/0.1.4.md")"; fi
if (cd "$tmp/build" && node scripts/whats-new.mjs) | grep -qx '0.1.4: from-the-tag'; then ok "stamp v0.1.4: What's new reads them"
else bad "stamp v0.1.4: What's new reads: $(cd "$tmp/build" && node scripts/whats-new.mjs 2>&1)"; fi
if in_dir "$tmp/build" stamp v0.1.4; then ok "stamp v0.1.4 again"; else bad "stamp v0.1.4 again: $(cat "$tmp/out")"; fi

# The publish job's checkout: the tag's commit, unstamped. The draft has the
# notes without its two blocks, and leaves the tree as `chant release publish`
# needs it: HEAD the release's commit, and nothing changed or untracked.
publish_ready() { # NAME DIR SHA: what chant's checkCheckoutIsRelease checks
  if [ "$(git -C "$2" rev-parse HEAD)" = "$3" ] && [ -z "$(git -C "$2" status --porcelain --untracked-files=normal)" ]; then ok "$1: the checkout is as a publish needs it"
  else bad "$1: HEAD $(git -C "$2" rev-parse HEAD), status: $(git -C "$2" status --porcelain --untracked-files=normal)"; fi
}
checkout "$tmp/publish" v0.1.4 "$pick"
publish_ready "draft v0.1.4: before" "$tmp/publish" "$pick"
if in_dir "$tmp/publish" draft v0.1.4; then ok "draft v0.1.4"; else bad "draft v0.1.4: $(cat "$tmp/out")"; fi
has "draft v0.1.4: from the tag" "v0.1.4's draft has the notes in v0.1.4's message"
if grep -qx '# Arugula next' "$tmp/release-body" && grep -qxF 'The fix (#101).' "$tmp/release-body" && ! grep -q 'whats-new\|no-notes\|from-the-tag\|Arugula-Deploy' "$tmp/release-body"; then ok "draft v0.1.4: the notes, without their blocks or the trailer"
else bad "draft v0.1.4's body: $(cat "$tmp/release-body")"; fi
publish_ready "draft v0.1.4: after" "$tmp/publish" "$pick"

# A tag from before #707 has no notes in its message: a title and maybe a
# trailer, as here. draft and notes read the tree at its commit, and stamp
# leaves the commit's file alone.
git checkout -q -b old "$one"
mkdir -p docs/releases && notes 0.0.8 headline && git add -A && git commit -qm old
old=$(git rev-parse HEAD)
git tag -a v0.0.8 -m "arugula 0.0.8" -m "Arugula-Deploy: dep_old" "$old"
git push -q "$forge" refs/tags/v0.0.8
git checkout -q later
checkout "$tmp/old" v0.0.8 "$old"
rm -f "$tmp/release-body"
if in_dir "$tmp/old" draft v0.0.8; then ok "draft v0.0.8"; else bad "draft v0.0.8: $(cat "$tmp/out")"; fi
has "draft v0.0.8: from the tree" "v0.0.8's draft has the notes in docs/releases/0.0.8.md"
if grep -qx 'notes 0.0.8' "$tmp/release-body" && ! grep -q 'whats-new\|Arugula-Deploy' "$tmp/release-body"; then ok "draft v0.0.8: without the headline section or the trailer"; else bad "draft v0.0.8's body: $(cat "$tmp/release-body")"; fi
publish_ready "draft v0.0.8" "$tmp/old" "$old"
if in_dir "$tmp/old" notes v0.0.8 && grep -qx 'notes 0.0.8' "$tmp/out"; then ok "notes v0.0.8: from the tree"; else bad "notes v0.0.8: $(cat "$tmp/out")"; fi
if in_dir "$tmp/old" stamp v0.0.8; then ok "stamp v0.0.8"; else bad "stamp v0.0.8: $(cat "$tmp/out")"; fi
has "stamp v0.0.8: the commit's file stays" "v0.0.8's message has no notes: docs/releases/0.0.8.md stays as the commit has it"
if [ -z "$(git -C "$tmp/old" status --porcelain -- docs/releases)" ]; then ok "stamp v0.0.8: docs/releases as the commit has it"; else bad "stamp v0.0.8: $(git -C "$tmp/old" status --porcelain -- docs/releases)"; fi
git tag -a v0.0.9 -m "arugula 0.0.9" "$one" && git push -q "$forge" refs/tags/v0.0.9
checkout "$tmp/none" v0.0.9 "$one"
rm -f "$tmp/release-body"
if in_dir "$tmp/none" draft v0.0.9 && [ ! -e "$tmp/release-body" ]; then ok "draft v0.0.9: no notes anywhere: keeps the default"; else bad "draft v0.0.9: $(cat "$tmp/out")"; fi
if in_dir "$tmp/none" notes v0.0.9; then bad "notes v0.0.9: $(cat "$tmp/out")"; else ok "notes v0.0.9: there are none"; fi
if in_dir "$tmp/none" stamp v0.0.9; then ok "stamp v0.0.9: no notes anywhere is a warning"; else bad "stamp v0.0.9: $(cat "$tmp/out")"; fi
has "stamp v0.0.9: says so" "warning: v0.0.9 has no notes"
# A tag GitHub doesn't have stops a build rather than shipping it without notes.
if in_dir "$tmp/build" stamp v9.9.9; then bad "stamp v9.9.9: $(cat "$tmp/out")"; else ok "stamp v9.9.9: a tag GitHub doesn't have stops it"; fi

# The next release can be cut from a commit older than the one that put the
# last release's notes on main (late is, here): its build still has v0.1.4's.
# In a shallow checkout too, as the macOS and Windows jobs' are, which stays so.
write "$tmp/same.md" "#104"
run 1 "older notes: a headline id v0.1.4's notes have, which late's tree doesn't" bump "$late" --notes "$tmp/same.md"
has "older notes: says which id" "the headline id from-the-tag"
sed 's/from-the-tag/from-the-next-tag/; s/From the tag/From the next tag/' "$tmp/same.md" >"$tmp/next.md"
run 0 "older notes: release v0.1.5 from late" bump "$late" --notes "$tmp/next.md"
has "older notes: v0.1.5 adds #104" "Too late (#104)"
if git cat-file -e "$late:docs/releases/0.1.4.md" 2>/dev/null; then bad "older notes: late has v0.1.4's notes"; else ok "older notes: late has no notes for v0.1.4"; fi
checkout "$tmp/shallow" v0.1.5 "$late" --depth=1
if in_dir "$tmp/shallow" stamp v0.1.5; then ok "older notes: stamp v0.1.5, shallow"; else bad "older notes: stamp v0.1.5: $(cat "$tmp/out")"; fi
has "older notes: says it wrote them" "wrote v0.1.4's notes into docs/releases/0.1.4.md, from its tag"
if (cd "$tmp/shallow" && node scripts/whats-new.mjs) | tr '\n' ' ' | grep -qF '0.1.5: from-the-next-tag 0.1.4: from-the-tag'; then ok "older notes: What's new has both"
else bad "older notes: What's new reads: $(cd "$tmp/shallow" && node scripts/whats-new.mjs 2>&1)"; fi
if [ "$(git -C "$tmp/shallow" rev-parse --is-shallow-repository)" = true ] && [ "$(git -C "$tmp/shallow" rev-parse HEAD)" = "$late" ]; then ok "older notes: the checkout stays shallow, on the commit"
else bad "older notes: shallow $(git -C "$tmp/shallow" rev-parse --is-shallow-repository), HEAD $(git -C "$tmp/shallow" rev-parse HEAD)"; fi
checkout "$tmp/deep" v0.1.5 "$late"
if in_dir "$tmp/deep" stamp v0.1.5 && [ -f "$tmp/deep/docs/releases/0.1.4.md" ] && [ "$(git -C "$tmp/deep" rev-parse --is-shallow-repository)" = false ]; then ok "older notes: stamp v0.1.5, in a full checkout, which stays full"
else bad "older notes: stamp v0.1.5, full: $(cat "$tmp/out")"; fi
main3=$(remote refs/heads/main)
git fetch -q "$forge" main

# The app's lane reads the same way, and only its own PRs: one that touches
# crates/desktop is listed, one that doesn't isn't. Main gets the notes alone.
git checkout -q -b app-later "$main3"
echo app >crates/desktop/app.txt && git add -A && git commit -qm "The app (#105)"
echo daemon >src/code && git commit -qam "Not the app (#106)"
app=$(git rev-parse HEAD)
git tag -a "ci/green-app/$app" -m green "$app"
git push -q "$forge" "$app:refs/heads/main" "refs/tags/ci/green-app/$app"
printf 'The app, with no word about the rest.\n' >"$tmp/app-short.md"
run 1 "app notes: a PR left out" bump --app "$app" --notes "$tmp/app-short.md"
has "app notes: names it" "leaves out #105"
if grep -q '^  #106' "$tmp/out"; then bad "app notes: asks about a PR that isn't the app's: $(cat "$tmp/out")"; else ok "app notes: only the app's PRs"; fi
printf '# The app\n\nIt changed (#105).\n' >"$tmp/app.md"
run 0 "app notes: release" bump --app "$app" --notes "$tmp/app.md"
has "app notes: which" "pushed app-v0.2.1 (${app:0:12})"
main4=$(remote refs/heads/main)
if [ "$(git rev-parse "$main4^" 2>/dev/null || { git fetch -q "$forge" main; git rev-parse "$main4^"; })" = "$app" ] && [ "$(git diff --name-only "$app" "$main4")" = docs/releases/app-0.2.1.md ]; then ok "app notes: the notes alone, on main"
else bad "app notes: main: $(git log --format='%h %s' -3 "$main4")"; fi
checkout "$tmp/app-build" app-v0.2.1 "$app"
if in_dir "$tmp/app-build" stamp app-v0.2.1; then ok "app notes: stamp"; else bad "app notes: stamp: $(cat "$tmp/out")"; fi
if diff -q "$tmp/app-build/docs/releases/app-0.2.1.md" "$tmp/app.md" >/dev/null; then ok "app notes: stamped as written"; else bad "app notes: stamped: $(cat "$tmp/app-build/docs/releases/app-0.2.1.md")"; fi
git checkout -q main

# check-version doesn't read the fixtures: a tag on a commit without the
# version's copy, that passed the rare jobs; and one that didn't pass them.
git tag -a v0.1.9 -m x "$four"
run 0 "check-version v0.1.9: no fixtures needed" check-version v0.1.9
git tag -a v0.1.8 -m x "$three"
run 1 "check-version v0.1.8: no rare jobs" check-version v0.1.8
run 2 "check-version: not a release tag" check-version vnext

# stamp: a tag's version into the checkout, as a release's build does it.
git tag -a v0.1.7 -m "arugula 0.1.7" "$two" && git tag -a app-v0.3.0 -m "arugula app 0.3.0" "$two"
git push -q "$forge" refs/tags/v0.1.7 refs/tags/app-v0.3.0
git checkout -q "$two"
run 0 "stamp v0.1.7" stamp v0.1.7
has "stamp v0.1.7: says what" "stamped v0.1.7: 0.1.0 -> 0.1.7 (daemon)"
if grep -q '^version = "0.1.7"' Cargo.toml && [ "$(grep -c '"0.1.7"' Cargo.lock)" = 2 ] &&
  grep -q 'arugula-core = { version = "0.1.7"' crates/daemon/Cargo.toml &&
  grep -A1 '"arugula-core"' crates/desktop/Cargo.lock | grep -q '0.1.7' && grep -q 'arugulad 0.1.7' THIRD_PARTY.md; then ok "stamp v0.1.7: the daemon's crates at 0.1.7"
else bad "stamp v0.1.7: $(git diff)"; fi
if grep -A1 '"serde"' Cargo.lock | grep -q '0.1.0' && grep -q 'serde = { version = "0.1.0" }' crates/daemon/Cargo.toml &&
  grep -q 'serde 0.1.0' THIRD_PARTY.md && grep -q '^version = "0.1.0"' crates/desktop/Cargo.toml; then ok "stamp v0.1.7: serde and the app keep their versions"
else bad "stamp v0.1.7 moved serde or the app"; fi
if [ "$(git diff --name-only | tr '\n' ' ')" = "Cargo.lock Cargo.toml THIRD_PARTY.md crates/daemon/Cargo.toml crates/desktop/Cargo.lock crates/desktop/THIRD_PARTY.md " ]; then ok "stamp v0.1.7: only version files"
else bad "stamp v0.1.7 changed: $(git diff --name-only)"; fi
before=$(git diff)
run 0 "stamp v0.1.7 again" stamp v0.1.7
if [ "$(git diff)" = "$before" ]; then ok "stamp v0.1.7 again: nothing more"; else bad "stamp v0.1.7 again changed more"; fi
git checkout -q -- .
git clean -fdq docs
run 0 "stamp app-v0.3.0" stamp app-v0.3.0
if grep -q '^version = "0.3.0"' crates/desktop/Cargo.toml && grep -A1 '"arugula-desktop"' crates/desktop/Cargo.lock | grep -q '0.3.0' &&
  grep -q '^version = "0.1.0"' Cargo.toml; then ok "stamp app-v0.3.0: the app alone"
else bad "stamp app-v0.3.0: $(git diff)"; fi
git checkout -q -- .
run 2 "stamp: not a release tag" stamp v1.2
git checkout -q main

exit $fail
