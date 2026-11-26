#!/usr/bin/env bash
# scripts/flaky-quarantine.mjs (Arugula #1423): a test that fails the merge
# queue's verdict in two groups within a day, for two pull requests that
# merged later, gets a `flaky` issue (a signed ci.flaky to the CI alerts
# URL) and a pull request quarantining it; one group stacked on the other,
# one pull request, a pull request that didn't merge, a day apart, a
# quarantined test, an open branch and a serial spec don't. With a fake gh
# that serves canned runs, jobs and logs and records what it's asked to
# write, a local origin, and a local server standing in for CI alerts.
#
#   scripts/tests/flaky-quarantine.sh
set -euo pipefail

src=$(cd "$(dirname "$0")/../.." && pwd)
tmp=$(mktemp -d)
srv=''
trap '[ -n "$srv" ] && kill "$srv" 2>/dev/null; rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

# What the logs say, as the jobs print them.
e2e_log='2026-11-17T06:57:50.1Z Running 64 tests using 1 worker
2026-11-17T06:57:50.2Z   1) [chrome] › e2e/docs-deleted.spec.ts:118:1 › a deleted Doc opens with Restore ──────
2026-11-17T06:57:50.3Z   1 failed
2026-11-17T06:57:50.4Z     [chrome] › e2e/docs-deleted.spec.ts:118:1 › a deleted Doc opens with Restore ─────────
2026-11-17T06:57:50.5Z   1 flaky
2026-11-17T06:57:50.6Z     [chrome] › e2e/move.spec.ts:180:1 › into a new project ───────────────────────────────
2026-11-17T06:57:50.7Z   63 passed (2.4m)'
rust_log='2026-11-17T06:57:50.1Z   TRY 1 FAIL [  10.351s] (─────────) arugulad::integration upgrade::shims_keep_panes
2026-11-17T06:57:50.2Z      Summary [ 418.562s] 2396 tests run: 2395 passed (1 flaky), 1 failed, 9 skipped
2026-11-17T06:57:50.3Z    FLAKY 2/2 [  19.633s] ( 831/2396) arugulad::integration upgrade::shims_keep_panes
2026-11-17T06:57:50.4Z   TRY 2 FAIL [  16.247s] (1085/2396) arugulad::integration forge_live::a_poke_polls
2026-11-17T06:57:50.5Z error: test run failed'

# The parsing, the stacking and the entry, on their own.
cat >"$tmp/unit.mjs" <<EOF
import { failures, decide, addEntry, quarantined } from "$src/scripts/flaky-quarantine.mjs";
const out = [];
const t = (what, good) => out.push((good ? "ok    " : "FAIL  ") + what);
const e2e = failures(process.env.E2E_LOG), rust = failures(process.env.RUST_LOG);
t("a spec that failed, without the rule after it",
  JSON.stringify(e2e) === JSON.stringify([{ phase: "e2e", file: "docs-deleted.spec.ts", test: "a deleted Doc opens with Restore" }]));
t("a nextest test that failed every try, without its binary; not the flaky one",
  JSON.stringify(rust) === JSON.stringify([{ phase: "test", test: "forge_live::a_poke_polls" }]));
const H = 3600e3, T = (h) => Date.UTC(2026, 10, 17) + h * H;
const g = (run, h, pr, merged, sha = "s" + run) => ({ run, url: "u" + run, at: T(h), pr, merged: merged === null ? null : T(merged), sha, tests: e2e });
t("two pull requests that merged: quarantined", decide([g(1, 0, 10, 1), g(2, 5, 11, 6)], T(7)).length === 1);
t("decided once both merged", decide([g(1, 0, 10, 1), g(2, 5, 11, 6)], T(7))[0]?.decided === T(6));
t("not before the second merged", decide([g(1, 0, 10, 1), g(2, 5, 11, 6)], T(5.5)).length === 0);
t("one pull request twice: not", decide([g(1, 0, 10, 1), g(2, 5, 10, 6)], T(7)).length === 0);
t("one that never merged: not", decide([g(1, 0, 10, 1), g(2, 5, 11, null)], T(7)).length === 0);
t("more than a day apart: not", decide([g(1, 0, 10, 1), g(2, 25, 11, 26)], T(27)).length === 0);
t("one group stacked on the other: not", decide([g(1, 0, 10, 1), g(2, 0.1, 11, 1)], T(2), () => false).length === 0);
t("older than two days: not", decide([g(1, 0, 10, 1), g(2, 5, 11, 6)], T(60)).length === 0);
const ws = process.env.WS;
const one = addEntry(ws, "e2e", { file: "a.spec.ts", test: "it works", issue: "7", until: "2026-12-01", why: "w" });
t("the first entry makes the quarantine", JSON.parse(one).ci.green.quarantine.phases.e2e[0].issue === "7" && JSON.parse(one).ci.green.quarantine.maxDays === 14);
t("and leaves the rest of the file as it was", one.startsWith(ws.slice(0, ws.indexOf('      "notify"'))) && one.includes('"events": ["ci.red", "ci.green"]'));
t("an entry a line", one.includes('            { "file": "a.spec.ts", "test": "it works", "issue": "7", "until": "2026-12-01", "why": "w" }'));
const two = addEntry(one, "test", { test: "x::y", issue: "8", until: "2026-12-01", why: "w" });
t("a second phase joins it", JSON.parse(two).ci.green.quarantine.phases.test[0].test === "x::y" && JSON.parse(two).ci.green.quarantine.phases.e2e.length === 1);
t("quarantined names both", [...quarantined(two)].join("|") === "a.spec.ts › it works|x::y");
console.log(out.join("\n"));
EOF
ws='{
  "ci": {
    "green": {
      "branch": "main",
      "phases": { "e2e": { "runs": ["e2e*"] } },
      "notify": {
        "events": ["ci.red", "ci.green"]
      }
    }
  },
  "agents": []
}'
E2E_LOG=$e2e_log RUST_LOG=$rust_log WS=$ws node "$tmp/unit.mjs" >"$tmp/unit" 2>&1 || bad "unit: $(cat "$tmp/unit")"
cat "$tmp/unit"
grep -q '^FAIL' "$tmp/unit" && fail=1

# gh: `api PATH` prints $FAKE/api/<PATH with / and ? as _>, or fails;
# `pr create` appends to $FAKE/calls and prints a link.
mkdir -p "$tmp/bin" "$tmp/api"
cat >"$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
case "$1 $2" in
  "pr create") printf '%s\n' "$*" >>"$FAKE/calls"; echo "https://forge/pull/99" ;;
  api*)
    shift
    [ "$1" = --allow-escape-sequences ] && shift
    f=$FAKE/api/$(printf '%s' "${1%%\?*}" | tr '/' '_')
    [ -f "$f" ] || { echo "fake gh: no $1" >&2; exit 1; }
    cat "$f"
    ;;
  *) echo "fake gh: $*" >&2; exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/gh"
api() { cat >"$tmp/api/$(printf '%s' "$1" | tr '/' '_')"; }
R=repos/forge/arugula
run() { # ID HOURS PR SHA: a failed group of check at 2026-11-17 + HOURS
  jq -n --argjson id "$1" --arg at "$(date -u -d "2026-11-17 +$2 hours" +%FT%TZ)" --arg b "gh-readonly-queue/main/pr-$3-base" --arg sha "$4" \
    '{id: $id, name: "check", html_url: "https://github.com/forge/arugula/actions/runs/\($id)", created_at: $at, head_branch: $b, head_sha: $sha}'
}
{ echo '{"workflow_runs": ['; run 1 0 10 aaa; echo ,; run 2 5 11 bbb; echo ,; run 3 5 12 ccc; echo ']}'; } | api "$R/actions/runs"
for id in 1 2 3; do echo "{\"jobs\": [{\"id\": ${id}0, \"name\": \"e2e (3)\", \"conclusion\": \"failure\"}, {\"id\": ${id}1, \"name\": \"lint\", \"conclusion\": \"success\"}]}" | api "$R/actions/runs/$id/jobs"; done
for id in 10 20; do printf '%s\n' "$e2e_log" | api "$R/actions/jobs/$id/logs"; done
echo 'nothing to see' | api "$R/actions/jobs/30/logs"
echo '{"merged_at": "2026-11-17T01:00:00Z"}' | api "$R/pulls/10"
echo '{"merged_at": "2026-11-17T06:00:00Z"}' | api "$R/pulls/11"
echo '{"merged_at": null}' | api "$R/pulls/12"
echo '{"status": "diverged"}' | api "$R/compare/aaa...bbb"

# CI alerts: checks the signature, keeps the body, answers issue 4242.
port=$(node -e 'const s=require("net").createServer().listen(0,()=>{console.log(s.address().port);s.close()})')
cat >"$tmp/alerts.mjs" <<'EOF'
import { createServer } from "node:http";
import { createHmac } from "node:crypto";
import { appendFileSync } from "node:fs";
createServer((q, r) => {
  let b = "";
  q.on("data", (c) => (b += c)).on("end", () => {
    const want = "sha256=" + createHmac("sha256", "k3y").update(`${q.headers["x-chant-timestamp"]}.${b}`).digest("hex");
    if (q.headers["x-chant-signature"] !== want) return r.writeHead(401).end("{}");
    appendFileSync(process.env.FAKE + "/events", b + "\n");
    r.writeHead(200, { "content-type": "application/json" }).end('{"did": "opened", "number": 4242}');
  });
}).listen(Number(process.argv[2]));
EOF
FAKE=$tmp node "$tmp/alerts.mjs" "$port" &
srv=$!

export PATH=$tmp/bin:$PATH FAKE=$tmp GITHUB_REPOSITORY=forge/arugula ARUGULA_CI_NOTIFY_KEY=k3y ARUGULA_CI_NOTIFY_URL=http://127.0.0.1:$port/ci/notify/p
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1
unset ARUGULA_QUARANTINE ARUGULA_QUARANTINE_TOKEN GITHUB_STEP_SUMMARY
git init -q --bare "$tmp/origin.git"
git init -q -b main "$tmp/work"
cd "$tmp/work"
git remote add origin "$tmp/origin.git"
mkdir -p web/e2e
printf '%s\n' "$ws" >chant.workspace.json
echo 'test("a deleted Doc opens with Restore", async () => {});' >web/e2e/docs-deleted.spec.ts
git add -A && git commit -qm base && git push -q origin main
for _ in $(seq 50); do curl -s -o /dev/null "http://127.0.0.1:$port" && break; sleep 0.1; done

quarantine() { # the script at 2026-11-17 07:00
  : >"$tmp/calls"
  : >"$tmp/events"
  ARUGULA_QUARANTINE_NOW=2026-11-17T07:00:00Z node "$src/scripts/flaky-quarantine.mjs" "$@" >"$tmp/out" 2>&1 || bad "exit $?: $(cat "$tmp/out")"
}

quarantine --dry-run
if grep -qF 'would quarantine docs-deleted.spec.ts › a deleted Doc opens with Restore (e2e): run 1 for #10, run 2 for #11' "$tmp/out"; then ok "a dry run says what it would do"; else bad "dry run: $(cat "$tmp/out")"; fi
if [ -s "$tmp/calls" ] || [ -s "$tmp/events" ]; then bad "a dry run wrote"; else ok "and does nothing"; fi

ARUGULA_QUARANTINE=off quarantine
if grep -qF 'ARUGULA_QUARANTINE is off' "$tmp/out" && [ ! -s "$tmp/calls" ]; then ok "off: leaves it"; else bad "off: $(cat "$tmp/out")"; fi

quarantine
b=quarantine-$(printf '%s' 'docs-deleted.spec.ts › a deleted Doc opens with Restore' | sha256sum | cut -c1-12)
if jq -e '.event == "ci.flaky" and .ci.test == "docs-deleted.spec.ts › a deleted Doc opens with Restore" and .ci.phase == "e2e" and (.ci.groups | map(.pr) == [10, 11]) and .run.repository == "forge/arugula"' "$tmp/events" >/dev/null; then
  ok "files the issue with a signed ci.flaky"
else bad "the event: $(cat "$tmp/events"); $(cat "$tmp/out")"; fi
if grep -qE "^pr create -R forge/arugula --head $b --title CI: quarantine docs-deleted.spec.ts › a deleted Doc opens with Restore, which ejected 2 merge groups --body part of Arugula #4242" "$tmp/calls"; then
  ok "opens the pull request, linking the issue without closing it"
else bad "pr create: $(cat "$tmp/calls"); $(cat "$tmp/out")"; fi
if grep -qE -- '--auto|pr merge' "$tmp/calls"; then bad "it queued it"; else ok "and leaves the merge to a person"; fi
pushed=$(git --git-dir="$tmp/origin.git" show "$b:chant.workspace.json" 2>/dev/null) || pushed=''
if jq -e '.ci.green.quarantine.phases.e2e[0] | .file == "docs-deleted.spec.ts" and .test == "a deleted Doc opens with Restore" and .issue == "4242" and .until == "2026-12-01" and (.why | contains("run 1 for #10, run 2 for #11"))' <<<"$pushed" >/dev/null; then
  ok "its branch quarantines the test, naming the issue"
else bad "the branch's file: $pushed"; fi
if [ "$(git rev-parse --abbrev-ref HEAD)" = HEAD ] && git diff --quiet; then ok "and leaves the checkout clean"; else bad "the checkout: $(git status --short)"; fi

quarantine
if grep -qF "$b exists already" "$tmp/out" && [ ! -s "$tmp/events" ]; then ok "its branch on origin: leaves it, filing nothing"; else bad "again: $(cat "$tmp/out")"; fi

git --git-dir="$tmp/origin.git" branch -q -D "$b"
git switch -q main
git merge -q --ff-only "$(git --git-dir="$tmp/origin.git" rev-parse main)" 2>/dev/null || true
printf '%s\n' "$pushed" >chant.workspace.json && git commit -qam quarantined
quarantine
if grep -qF 'no test ejected two merge groups' "$tmp/out" && [ ! -s "$tmp/events" ]; then ok "quarantined already: leaves it"; else bad "quarantined: $(cat "$tmp/out")"; fi

git reset -q --hard HEAD^
echo 'test.describe.configure({ mode: "serial" });' >>web/e2e/docs-deleted.spec.ts && git commit -qam serial
quarantine
if grep -qF 'its spec is serial' "$tmp/out" && [ ! -s "$tmp/events" ] && [ ! -s "$tmp/calls" ]; then ok "a serial spec: warns, and leaves it"; else bad "serial: $(cat "$tmp/out")"; fi
git reset -q --hard HEAD^

echo '{"status": "ahead"}' | api "$R/compare/aaa...bbb"
quarantine
if grep -qF 'no test ejected two merge groups' "$tmp/out"; then ok "the second group stacked on the first: one break, leaves it"; else bad "stacked: $(cat "$tmp/out")"; fi

exit $fail
