#!/usr/bin/env bash
# Notes on every pull request (#722) and the release train (#724), in a small
# repository: a release's notes come from the notes its range added, the check
# fails a pull request that says nothing, and the train goes out only when it has
# something to say. A fake `scripts/release` and gh stand in for the rest.
#
#   scripts/tests/release-notes.sh
set -euo pipefail
src=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
unset GITHUB_STEP_SUMMARY ARUGULA_RELEASE_TRAIN

git init -q --bare "$tmp/origin.git"
git init -q -b main "$tmp/work"
cd "$tmp/work"
git remote add origin "$tmp/origin.git"
mkdir -p scripts docs/notes docs/releases web/src crates/foo/src
cp "$src/release-notes.mjs" "$src/release-note-check.mjs" "$src/pr-notes-check" "$src/release-train" scripts/
echo '# notes' >docs/notes/README.md
git add -A && git commit -qm base
git tag v1.2.3

note() { # NAME TITLE [EXTRA FIELD LINE]
  printf -- '---\nsection: Changed\ntitle: %s\nline: %s does a thing.\n%s---\n' "$2" "$2" "${3:-}" >"docs/notes/$1.md"
}
land() { # SUBJECT: everything changed, as main's next commit
  git add -A
  git commit -qm "$1" --allow-empty
}
echo 1 >web/src/a.ts
note first-thing "First thing" 'try: {"route": "/chat"}
needs: control
'
land "The first thing (#10)"
echo 2 >web/src/a.ts
land "A change nobody notices (#11)"
note small-fix "Small fix" 'headline: no
'
echo 3 >web/src/a.ts
land "A small fix (#12)"
mkdir -p crates/proto/fixtures/releases/1.2.3
echo x >crates/proto/fixtures/releases/1.2.3/a.jsonl
land "Release record: what v1.2.3 shipped"
to=$(git rev-parse HEAD)

gen() { node scripts/release-notes.mjs "$@"; }
out=$(gen v1.2.3 "$to" 1.2.4)
has() { # WHAT TEXT
  if [[ "$out" == *"$2"* ]]; then ok "$1"; else bad "$1: wanted '$2' in: $out"; fi
}
lacks() { # WHAT TEXT
  if [[ "$out" == *"$2"* ]]; then bad "$1: didn't want '$2' in: $out"; else ok "$1"; fi
}
has "the notes have a title" "# Arugula 1.2.4"
has "a note names its pull request" "**First thing** (#10)"
has "a pull request with no note is listed" '```no-notes'
has "by number" "#11"
lacks "what only records a release isn't listed" "Release record"
# shellcheck disable=SC2016
block=$(printf '%s\n' "$out" | sed -n '/^```whats-new$/,/^```$/p' | sed '1d;$d')
if [ "$(jq -r '.[0].id' <<<"$block")" = first-thing ]; then ok "a headline has the note's id"; else bad "headline: $block"; fi
if [ "$(jq -r '.[0].try.route' <<<"$block")" = /chat ]; then ok "and its try"; else bad "try: $block"; fi
if [ "$(jq -r '.[0].needs' <<<"$block")" = control ]; then ok "and who it needs"; else bad "needs: $block"; fi
if [ "$(jq 'length' <<<"$block")" = 1 ]; then ok "headline: no stays out of What's new"; else bad "headlines: $block"; fi
has "but in the notes" "**Small fix** (#12)"

# Nothing to say.
code=0
gen v1.2.3 v1.2.3 1.2.4 >/dev/null 2>&1 || code=$?
if [ "$code" = 3 ]; then ok "no notes in the range: exits 3"; else bad "exit $code"; fi

# A release whose notes all say headline: no still gets a headline.
git tag from-here "$to"
note just-small "Just small" 'headline: no
'
land "Just small (#13)"
out=$(gen from-here HEAD 1.2.5)
# shellcheck disable=SC2016
block=$(printf '%s\n' "$out" | sed -n '/^```whats-new$/,/^```$/p' | sed '1d;$d')
if [ "$(jq -r '.[0].id' <<<"$block")" = just-small ]; then ok "with no headline wanted, the first note serves"; else bad "$block"; fi

# A note it can't read.
printf 'no fence here\n' >docs/notes/broken.md
land "Broken (#14)"
if gen from-here HEAD 1.2.5 >/dev/null 2>&1; then bad "a broken note should fail"; else ok "a note it can't read fails the build of the notes"; fi
git rm -q docs/notes/broken.md
land "Not broken (#15)"

# The check on a pull request.
main=$(git rev-parse HEAD)
pr() { # BRANCH: a side branch from main
  git switch -q -c "$1" "$main"
  mkdir -p crates/foo/src web/src
}
passes() { # WHAT
  if scripts/pr-notes-check "$main" HEAD >"$tmp/out" 2>&1; then ok "$1"; else bad "$1: $(cat "$tmp/out")"; fi
}
refuses() { # WHAT
  if scripts/pr-notes-check "$main" HEAD >"$tmp/out" 2>&1; then bad "$1 should fail: $(cat "$tmp/out")"; else ok "$1"; fi
}
pr says-nothing
echo x >web/src/b.ts
git add -A && git commit -qm "A change"
refuses "a product change with neither: fails"
if grep -qF 'Release-note: none' "$tmp/out"; then ok "and says how to fix it"; else bad "the help: $(cat "$tmp/out")"; fi
git commit -q --allow-empty -m "Oh right

Release-note: none, the fake forge only"
passes "a commit says why none: passes"
pr says-none-with-no-reason
echo x >web/src/c.ts
git add -A && git commit -qm "A change

Release-note: none"
refuses "none with no reason: fails"
pr adds-a-note
echo x >crates/foo/src/lib.rs
note new-thing "New thing"
git add -A && git commit -qm "A change"
passes "a valid note: passes"
pr adds-a-broken-note
echo x >crates/foo/src/lib2.rs
printf -- '---\nsection: Nope\ntitle: T\nline: L\n---\n' >docs/notes/bad-section.md
git add -A && git commit -qm "A change"
refuses "an invalid note: fails"
pr repeats-an-id
echo x >crates/foo/src/lib3.rs
# shellcheck disable=SC2016
printf '```whats-new\n[{"id": "used-id", "title": "T", "line": "L"}]\n```\n' >docs/releases/1.0.0.md
note used-id "Used"
git add -A && git commit -qm "A change"
refuses "an id an earlier release used: fails"
pr docs-only
echo x >docs/guide.md
git add -A && git commit -qm "Docs"
passes "docs alone: needs none"
# A note alone is checked too (#795): the train reads every note, and one it
# can't read holds every release. The later line: wins, as in parseNote.
pr notes-only-too-long
note too-long "Too long" "line: $(printf 'x%.0s' {1..245})
"
git add -A && git commit -qm "Notes"
refuses "a note alone still has to read: a 245-character line fails"
if grep -qF 'docs/notes/too-long.md: line is over 240 characters' "$tmp/out"; then ok "naming the file"; else bad "the message: $(cat "$tmp/out")"; fi
# A note already released can be edited: its id is in a release by then.
pr edits-a-released-note
mkdir -p docs/releases
# shellcheck disable=SC2016
printf '```whats-new\n[{"id": "shipped", "title": "T", "line": "L"}]\n```\n' >docs/releases/1.0.1.md
note shipped "Shipped"
git add -A && git commit -qm "Shipped (#16)"
released=$(git rev-parse HEAD)
note shipped "Shipped, reworded"
git add -A && git commit -qm "Reword"
if scripts/pr-notes-check "$released" HEAD >"$tmp/out" 2>&1; then ok "editing a released note: passes"; else bad "editing a released note: $(cat "$tmp/out")"; fi
pr tests-only
mkdir -p crates/foo/tests web/e2e .github/workflows
echo x >crates/foo/tests/t.rs
echo x >web/e2e/a.spec.ts
echo x >.github/workflows/w.yml
git add -A && git commit -qm "Tests"
passes "tests and CI alone: need none"
git switch -q main

# The train. A fake release: `last-green` prints the commit, `bump` records itself.
cat >"$tmp/release" <<'RELEASE'
#!/usr/bin/env bash
case "$1" in
  last-green) echo "${FAKE_GREEN:-}" ;;
  bump)
    shift
    printf 'bump %s\n' "$*" >>"$FAKE/bumps"
    while [ "$#" != 0 ]; do
      if [ "$1" = --notes ]; then cp "$2" "$FAKE/notes"; fi
      shift
    done
    exit "${FAKE_BUMP_EXIT:-0}"
    ;;
esac
RELEASE
cat >"$tmp/gh" <<'GH'
#!/usr/bin/env bash
echo "${FAKE_GOING:-0} ${FAKE_STARTED:-}"
GH
mkdir "$tmp/bin"
mv "$tmp/gh" "$tmp/bin/gh"
chmod +x "$tmp/release" "$tmp/bin/gh"
# A hand cut's file reaches main before its tag: its headlines stay out of a
# train's notes, and only a file newer than the tag counts.
hand() { # VERSION IDS (a JSON array of strings)
  mkdir -p docs/releases
  # shellcheck disable=SC2016 # node's script, in single quotes on purpose
  node -e 'const [v, ids] = process.argv.slice(1); console.log("# Arugula " + v + "\n\n```whats-new\n" + JSON.stringify(JSON.parse(ids).map((id) => ({ id, title: id, line: id }))) + "\n```")' "$1" "$2" >"docs/releases/$1.md"
}
hand 1.3.0 '["first-thing"]'
out=$(gen v1.2.3 "$to" 1.2.4 2>/dev/null)
lacks "an untagged hand cut's headline stays out" "First thing"
has "the rest still go" "Small fix"
code=0
gen v1.2.3 "$to" 1.2.4 >/dev/null 2>"$tmp/err" || code=$?
if grep -qF "first-thing is in 1.3.0's notes" "$tmp/err"; then ok "says what it left out"; else bad "said: $(cat "$tmp/err")"; fi
hand 1.3.0 '["first-thing", "small-fix"]'
code=0
gen v1.2.3 "$to" 1.2.4 >/dev/null 2>&1 || code=$?
if [ "$code" = 3 ]; then ok "the hand cut has them all: nothing to say"; else bad "exit $code, wanted 3"; fi
rm docs/releases/1.3.0.md
hand 1.2.0 '["first-thing"]'
out=$(gen v1.2.3 "$to" 1.2.4 2>/dev/null)
has "an older release's file leaves notes alone" "First thing"
rm docs/releases/1.2.0.md
hand 1.2.4 '["first-thing"]'
out=$(gen v1.2.3 "$to" 1.2.4 2>/dev/null)
has "its own version's file leaves notes alone" "First thing"
rm docs/releases/1.2.4.md

git push -q origin main --tags
export FAKE=$tmp PATH=$tmp/bin:$PATH ARUGULA_TRAIN_RELEASE=$tmp/release
train() { # WHAT WANT_EXIT WANT_BUMPS SAYS
  : >"$tmp/bumps"
  local code=0 n
  scripts/release-train >"$tmp/out" 2>&1 || code=$?
  n=$(grep -c . "$tmp/bumps" || true)
  if [ "$code" != "$2" ]; then bad "$1: exit $code: $(cat "$tmp/out")"
  elif [ "$n" != "$3" ]; then bad "$1: bumped $n times: $(cat "$tmp/out")"
  elif ! grep -qF -- "$4" "$tmp/out"; then bad "$1: said: $(cat "$tmp/out")"
  else ok "$1"; fi
}
export FAKE_GREEN=$main
train "off by default: stays" 0 0 "isn't on"
export ARUGULA_RELEASE_TRAIN=on
FAKE_GOING=1 train "a release going: stays" 0 0 "already going"
FAKE_GREEN='' train "nothing green: stays" 0 0 "no commit has"
recent=$(node -e 'console.log(new Date(Date.now() - 30 * 60000).toISOString())')
FAKE_STARTED=$recent train "a release 30m ago: waits for the gap" 0 0 "waits for 120m"
FAKE_STARTED=$recent ARUGULA_TRAIN_GAP_MINUTES=20 FAKE_GREEN='' train "past a shorter gap: goes on" 0 0 "no commit has"
FAKE_STARTED=$recent ARUGULA_TRAIN_GAP_MINUTES=0 FAKE_GREEN='' train "a manual run: no gap" 0 0 "no commit has"
git tag -f v1.2.9 "$main" >/dev/null
git push -q -f origin --tags
train "already released: stays" 0 0 "nothing new"
empty=$(git commit-tree "$(git rev-parse "$main^{tree}")" -p "$main" -m "Nobody notices (#16)")
git update-ref refs/heads/main "$empty"
git push -q origin main
FAKE_GREEN=$empty train "nothing to say: stays" 0 0 "added a note"
git switch -q main 2>/dev/null || true
git reset -q --hard "$empty"
note worth-saying "Worth saying"
land "Worth saying (#17)"
worth=$(git rev-parse HEAD)
git push -q origin main
FAKE_GREEN=$worth train "something to say: goes out" 0 1 "as v1.2.10"
if grep -qF "bump $worth --notes" "$tmp/bumps" && grep -qF -- '--rare' "$tmp/bumps"; then ok "with its commit, the notes and the rare jobs"; else bad "bumps: $(cat "$tmp/bumps")"; fi
if grep -qF '# Arugula 1.2.10' "$tmp/notes"; then ok "under the next patch's number"; else bad "notes: $(cat "$tmp/notes")"; fi
FAKE_BUMP_EXIT=1 FAKE_GREEN=$worth train "a failed gate: held, and red" 1 1 "is held"

exit $fail
