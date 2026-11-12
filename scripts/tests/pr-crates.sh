#!/usr/bin/env bash
# Which Rust tests a pull request's own run takes (scripts/pr-crates, #917):
# in a small repository with three crates, one of them reading a file
# outside itself and one excluded from the workspace, a merge commit's files
# decide it, and the workspace's own config takes everything.
#
#   scripts/tests/pr-crates.sh
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
mkdir -p scripts docs web/src crates/core/src crates/daemon/src crates/cli/src crates/desktop/src
cp "$src/scripts/pr-crates" scripts/
cat >Cargo.toml <<'TOML'
[workspace]
members = ["crates/*"]
exclude = [
    "crates/desktop",
]

[workspace.dependencies]
my-core = { path = "crates/core" }
TOML
crate() {
  printf '[package]\nname = "%s"\n\n[dependencies]\n' "$2" >"crates/$1/Cargo.toml"
  echo '// nothing' >"crates/$1/src/lib.rs"
}
crate core my-core
crate daemon my-daemon
crate cli my-cli
crate desktop my-desktop
# The daemon's tests read the docs' permissions and the core's sources.
cat >crates/daemon/src/lib.rs <<'RS'
const PERMISSIONS: &str = include_str!("../../../docs/permissions.json");
const CORE: &str = "../core/src/";
RS
: >docs/permissions.json
: >docs/guide.md
: >web/src/app.ts
git add -A && git commit -qm base

# plan PATH...: touches each path on a side branch, merges it as Actions' merge
# ref does, and leaves scripts/pr-crates' answer in $tmp/out.
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
  scripts/pr-crates >"$tmp/out"
  git checkout -q main
  git branch -qD side
}
has() {
  local what=$1 line=$2
  if grep -qxF -- "$line" "$tmp/out"; then ok "$what"; else bad "$what: wanted '$line' in: $(cat "$tmp/out")"; fi
}
tests_have() {
  local what=$1 bit=$2
  if grep '^tests=' "$tmp/out" | grep -qF -- "$bit"; then ok "$what"; else bad "$what: $(cat "$tmp/out")"; fi
}
tests_lack() {
  local what=$1 bit=$2
  if grep '^tests=' "$tmp/out" | grep -qF -- "$bit"; then bad "$what: $(cat "$tmp/out")"; else ok "$what"; fi
}

plan docs/guide.md web/src/app.ts
has "docs and web alone: no Rust" "rust=false"
has "and no tests" "tests=none()"

plan crates/cli/src/lib.rs
has "a crate: Rust" "rust=true"
has "that crate and what depends on it" "tests=package(=my-cli) | rdeps(=my-cli)"

plan crates/core/src/lib.rs
tests_have "the crate itself" "rdeps(=my-core)"
tests_have "a crate that names its sources" "rdeps(=my-daemon)"
tests_lack "not one that doesn't" "my-cli"

plan docs/permissions.json
has "a file a crate reads: Rust" "rust=true"
has "that crate's tests" "tests=package(=my-daemon) | rdeps(=my-daemon)"

plan crates/desktop/src/lib.rs
has "the excluded crate is no workspace crate's" "rust=false"

plan Cargo.lock
has "the lock file: everything" "tests=all()"

plan .cargo/config.toml
has "cargo's config: everything" "tests=all()"

plan crates/core/src/lib.rs .github/workflows/pull-request.yml
has "this run's own workflow: everything" "tests=all()"

# Can't tell: everything.
git checkout -q --detach main
scripts/pr-crates no-such-commit HEAD >"$tmp/out"
git checkout -q main
has "a diff that fails: everything" "tests=all()"

exit $fail
