#!/usr/bin/env bash
# scripts/quarantine.mjs (.config/quarantine.toml): `check` refuses an
# entry without an issue, past its `until` or more than 14 days out, a
# test that isn't there, and nextest configs that don't match the
# manifest; `write` sets their default-filter per workspace; the specs'
# grep matches the quarantined test only. Against a small fake checkout.
#
#   scripts/tests/quarantine.sh
set -euo pipefail

q=$(cd "$(dirname "$0")/.." && pwd)/quarantine.mjs
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

# Two workspaces, as the checkout has: the root's crates/* but the server's
# crate, which server/Cargo.toml has.
cd "$tmp"
mkdir -p .config server/.config crates/d/src crates/d/tests/integration crates/srv/src web/e2e
printf '[workspace]\nmembers = ["crates/*"]\nexclude = [\n    "crates/srv",\n]\n' >Cargo.toml
printf '[workspace]\nmembers = [\n    "../crates/srv",\n]\n' >server/Cargo.toml
printf '[package]\nname = "d"\n' >crates/d/Cargo.toml
: >crates/d/src/lib.rs
printf '#[test]\nfn flaky_one() {}\n' >crates/d/tests/integration/main.rs
printf '[package]\nname = "srv"\n' >crates/srv/Cargo.toml
printf 'mod tests { mod feedback { #[test] fn round_three() {} } }\n' >crates/srv/src/lib.rs
printf 'test("a flaky spec", async () => {});\ntest("a flaky spec too", async () => {});\n' >web/e2e/a.spec.ts
for c in .config/nextest.toml server/.config/nextest.toml; do
  printf '[profile.ci]\nretries = 1\ndefault-filter = "all()"\n' >"$c"
done
export ARUGULA_QUARANTINE_ROOT=$tmp ARUGULA_QUARANTINE_TODAY=2026-10-27

entry() { # KIND NAME ISSUE UNTIL: one manifest entry
  if [ "$1" = rust ]; then
    printf '[[rust]]\ntest = "%s"\nissue = "%s"\nuntil = "%s"\nwhy = "Flaky."\n\n' "$2" "$3" "$4"
  else
    printf '[[e2e]]\nspec = "web/e2e/a.spec.ts"\ntitle = "%s"\nissue = "%s"\nuntil = "%s"\nwhy = "Flaky."\n\n' "$2" "$3" "$4"
  fi
}
good() {
  entry rust "d::integration flaky_one" 12 2026-11-10
  entry rust "srv feedback::round_three" "https://github.com/arugula-salad/arugula/issues/34" 2026-10-27
  entry e2e "a flaky spec" "#56" 2026-10-30
}
same() { # NAME WANT GOT
  if [ "$2" = "$3" ]; then ok "$1"; else bad "$1: wanted '$2', got '$3'"; fi
}
check() { # NAME WANT(pass|TEXT): check's exit and, failing, its message
  if node "$q" check >"$tmp/out" 2>&1; then
    if [ "$2" = pass ]; then ok "$1"; else bad "$1: passed, wanted '$2'"; fi
  elif [ "$2" = pass ]; then
    bad "$1: $(cat "$tmp/out")"
  elif grep -qF -- "$2" "$tmp/out"; then
    ok "$1"
  else
    bad "$1: wanted '$2': $(cat "$tmp/out")"
  fi
}

check "no manifest: nothing quarantined" pass

good >.config/quarantine.toml
check "the configs not written yet" "run \`just quarantine\`"
node "$q" write
same "write: the root's test into the root's config" \
  'default-filter = "not ((binary_id(=d::integration) & test(=flaky_one)))"' "$(grep default-filter .config/nextest.toml)"
same "write: the server's into the server's" \
  'default-filter = "not ((binary_id(=srv) & test(=feedback::round_three)))"' "$(grep default-filter server/.config/nextest.toml)"
check "written, issues, every until in reach" pass
same "nextest root: the set alone" "(binary_id(=d::integration) & test(=flaky_one))" "$(node "$q" nextest root)"
same "e2e: the spec" "web/e2e/a.spec.ts › a flaky spec" "$(node "$q" e2e)"

# The spec's grep, against Playwright's title paths (project, file relative
# to web/e2e, describes, title, tags).
grep=$(Q=$q node --input-type=module -e '
  const { e2eGrep } = await import(process.env.Q);
  const [re] = e2eGrep();
  const t = (s) => re.test(s);
  console.log([
    t("chrome a.spec.ts a flaky spec"),
    t("chrome a.spec.ts group a flaky spec @slow"),
    t("chrome a.spec.ts a flaky spec too"),
    t("chrome b/a.spec.ts a flaky spec"),
  ].join(" "));
')
same "grep: the test, in a describe and tagged, not a longer title or another file" "true true false false" "$grep"

# What makes lint fail.
entry rust "d::integration flaky_one" "" 2026-11-01 >.config/quarantine.toml
node "$q" write
check "an empty issue" "no issue"
printf '[[rust]]\ntest = "d::integration flaky_one"\nuntil = "2026-11-01"\nwhy = "Flaky."\n' >.config/quarantine.toml
check "no issue at all" "no issue"
entry rust "d::integration flaky_one" 12 2026-10-26 >.config/quarantine.toml
check "a day past until" "expired on 2026-10-26"
entry rust "d::integration flaky_one" 12 2026-11-10 >.config/quarantine.toml
check "14 days out" pass
entry rust "d::integration flaky_one" 12 2026-11-11 >.config/quarantine.toml
check "15 days out" "more than 14 days out"
entry rust "d::integration flaky_one" 12 2026-02-30 >.config/quarantine.toml
check "not a day" "isn't a date"
entry rust "d::integration flaky_one" "soon" 2026-11-01 >.config/quarantine.toml
check "an issue that's neither a number nor a URL" "isn't a number or a URL"
{ entry rust "d::integration flaky_one" 12 2026-11-01; entry rust "d::integration flaky_one" 13 2026-11-01; } >.config/quarantine.toml
check "the same test twice" "twice"

# Tests that aren't there (a stale or mistyped entry; nextest refuses a
# binary id that isn't, in every profile).
entry rust "d::integration gone" 12 2026-11-01 >.config/quarantine.toml
check "a test that isn't" "no such test in crates/d"
entry rust "nope::integration flaky_one" 12 2026-11-01 >.config/quarantine.toml
check "a package that isn't" "no package nope"
entry rust "d::other flaky_one" 12 2026-11-01 >.config/quarantine.toml
check "a binary that isn't" "no test binary d::other"
entry rust "flaky_one" 12 2026-11-01 >.config/quarantine.toml
check "a name without its binary" "BINARY-ID TEST-NAME"
entry e2e "no such title" 12 2026-11-01 >.config/quarantine.toml
check "a spec title that isn't" "no test titled that"
entry e2e "a flaky spec" 12 2026-11-01 | sed 's#web/e2e/a.spec.ts#web/e2e/b.spec.ts#' >.config/quarantine.toml
check "a spec that isn't" "no spec web/e2e/b.spec.ts"
printf 'test.describe.configure({ mode: "serial" });\ntest("a serial step", async () => {});\n' >web/e2e/s.spec.ts
entry e2e "a serial step" 12 2026-11-01 | sed 's#web/e2e/a.spec.ts#web/e2e/s.spec.ts#' >.config/quarantine.toml
check "a test in a serial spec" "runs serially"
rm web/e2e/s.spec.ts

# The manifest's format.
printf '[[rust]]\ntest = d::integration flaky_one\n' >.config/quarantine.toml
check "an unquoted value" "not \`key = \"string\"\`"
printf '[[unit]]\ntest = "x"\n' >.config/quarantine.toml
check "a table that isn't" "only [[rust]] and [[e2e]]"
entry rust "d::integration flaky_one" 12 2026-11-01 | sed 's/^why/reason/' >.config/quarantine.toml
check "a key that isn't" "has no reason"

# Back to none: write leaves every test in.
: >.config/quarantine.toml
node "$q" write
check "empty again" pass
same "write: all() for none" 'default-filter = "all()"' "$(grep default-filter .config/nextest.toml)"

exit "$fail"
