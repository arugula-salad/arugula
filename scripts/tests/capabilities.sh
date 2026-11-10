#!/usr/bin/env bash
# The decision and feature records' checks. `just decisions check` fails on a
# standing decision naming a path that isn't there, but not on a superseded
# one, and not on a feature record's (a PR owes those nothing); it fails on a
# malformed record and a stale index. `just map` reports what's behind and
# gone. Against a small fake checkout.
#
#   scripts/tests/capabilities.sh
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

cd "$tmp"
git init -q -b main
git config user.email t@example.com
git config user.name t
mkdir -p scripts docs/decisions docs/capabilities crates/a/src
cp "$here"/decisions-index.mjs "$here"/capabilities.mjs "$here"/map.mjs scripts/
: >crates/a/src/lib.rs
decision() { # file state path
  printf -- '---\nid: "%s"\ntitle: "T"\narea: "A"\nstate: "%s"\nconstrains:\n  - "path:%s"\n---\nBody\n' "${1%%-t*}" "$2" "$3" >"docs/decisions/$1.md"
}
decision aru-001-t decided crates/a/src/lib.rs
git add -A
git commit -qm one
first=$(git rev-parse HEAD)
record() { # file verified entry
  printf -- '---\nid: "%s"\ntitle: "R"\nverified_through: "%s"\nentry: ["%s"]\npaths: []\nshared: []\ntests: ["crates/a/src/lib.rs"]\ndecisions: ["aru-001"]\n---\n# R\n\n## What a person can do\n\n## Where it runs\n\n## Compatibility\n\n## Not known\n' "$1" "$2" "$3" >"docs/capabilities/$1.md"
}
record rec "$first" crates/a/src/lib.rs
node scripts/decisions-index.mjs
node scripts/capabilities.mjs

check() { node scripts/decisions-index.mjs --check 2>&1 && node scripts/capabilities.mjs --check 2>&1; }

if check >/dev/null; then ok "a clean checkout passes"; else bad "a clean checkout should pass"; fi

decision aru-002-t decided crates/gone.rs
node scripts/decisions-index.mjs
if out=$(check); then bad "a dead path in a standing decision should fail"; else
  case $out in *"aru-002: crates/gone.rs"*) ok "a dead path in a standing decision fails, naming it" ;; *) bad "wrong message: $out" ;; esac
fi

decision aru-002-t superseded crates/gone.rs
node scripts/decisions-index.mjs
if check >/dev/null; then ok "a superseded decision may name a path that's gone"; else bad "a superseded decision shouldn't fail"; fi

record moved "$first" crates/moved.rs
node scripts/capabilities.mjs
if check >/dev/null; then ok "a feature record naming a moved file doesn't fail"; else bad "a moved file shouldn't fail a PR"; fi

printf '\n' >>docs/capabilities/README.md
if check >/dev/null 2>&1; then bad "a stale index should fail"; else ok "a stale index fails"; fi
node scripts/capabilities.mjs

record broken nothex crates/a/src/lib.rs
if node scripts/capabilities.mjs --check >/dev/null 2>&1; then bad "a malformed record should fail"; else ok "a malformed record fails"; fi
rm docs/capabilities/broken.md

echo '// more' >>crates/a/src/lib.rs
git add -A
git commit -qm two
out=$(node scripts/map.mjs --no-ci)
case $out in *"rec "*"1 behind"*) ok "the map counts commits since verified_through" ;; *) bad "no behind count: $out" ;; esac
case $out in *"gone: crates/moved.rs"*) ok "the map reports a record's dead path" ;; *) bad "no dead path: $out" ;; esac

exit $fail
