#!/usr/bin/env bash
# What a pull request's own run takes (scripts/pr-plan,
# web/scripts/e2e-select.mjs): in a small repository with this script and a few
# specs, a merge commit's files decide it, and failing to tell runs them all.
#
#   scripts/tests/pr-plan.sh
set -euo pipefail

src=$(cd "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1

w=$tmp/work
git init -q -b main "$w"
cd "$w"
mkdir -p scripts web/scripts web/e2e web/src docs crates/platform-chat/src crates/control-wire/src
cp "$src/scripts/pr-plan" scripts/
cp "$src/web/scripts/e2e-select.mjs" web/scripts/
for s in journey-j1 layout board-find board-labels chat-block control; do : >"web/e2e/$s.spec.ts"; done
: >web/e2e/board-find.webkit.spec.ts
: >web/e2e/helpers.ts
cat >web/e2e/durations.json <<'JSON'
{"journey-j1.spec.ts": 20, "layout.spec.ts": 20, "board-find.spec.ts": 20, "board-labels.spec.ts": 20, "chat-block.spec.ts": 20, "control.spec.ts": 2000}
JSON
git add -A && git commit -qm base

# plan PATH...: touches each path on a side branch, merges it as Actions' merge
# ref does, and leaves scripts/pr-plan's answer in $tmp/out.
plan() {
  git checkout -q -b side main
  local p
  for p in "$@"; do
    mkdir -p "$(dirname "$p")"
    echo changed >>"$p"
  done
  git add -A && git commit -qm side
  git checkout -q --detach main
  git merge -q --no-ff --no-edit side
  scripts/pr-plan >"$tmp/out"
  git checkout -q main
  git branch -qD side
}
has() {
  local what=$1 line=$2
  if grep -qxF -- "$line" "$tmp/out"; then ok "$what"; else bad "$what: wanted '$line' in: $(cat "$tmp/out")"; fi
}
only_has() {
  local what=$1 spec=$2
  if grep '^only=' "$tmp/out" | grep -qF -- "$spec"; then ok "$what"; else bad "$what: $(cat "$tmp/out")"; fi
}
only_lacks() {
  local what=$1 spec=$2
  if grep '^only=' "$tmp/out" | grep -qF -- "$spec"; then bad "$what: $(cat "$tmp/out")"; else ok "$what"; fi
}

plan docs/guide.md AGENTS.md
has "docs alone: no e2e" "e2e=false"

plan web/src/board-row.tsx
has "web: some specs" "e2e=true"
only_has "the specs named like the change" board-find.spec.ts
only_has "a plural in a path finds them too" board-labels.spec.ts
only_has "the ones that always run" journey-j1.spec.ts
only_lacks "not the rest" chat-block.spec.ts
only_lacks "not a WebKit spec" webkit

plan crates/platform-chat/src/rooms.rs
only_has "a crate's name counts" chat-block.spec.ts

plan crates/control-wire/src/lib.rs
has "a wire crate: some specs, so one shard" "shards=[1]"
only_lacks "and control's words say nothing" control.spec.ts

plan web/e2e/control.spec.ts
has "a changed spec runs, whatever the rest" "e2e=true"
has "past 70% of the run's time, all of them" "only="
has "in twelve shards" "shards=[1,2,3,4,5,6,7,8,9,10,11,12]"

plan web/e2e/helpers.ts
has "what every spec reads: all of them" "only="

# Can't tell: every spec.
mv web/scripts/e2e-select.mjs "$tmp/selector"
plan web/src/board-row.tsx
has "no answer from the selection: all of them" "shards=[1,2,3,4,5,6,7,8,9,10,11,12]"
mv "$tmp/selector" web/scripts/e2e-select.mjs

exit $fail
