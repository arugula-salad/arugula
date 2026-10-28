#!/usr/bin/env bash
# scripts/ci-nightly-red: one "nightly is red: WORKFLOW" issue per
# scheduled workflow, opened when its newest scheduled run fails, commented
# on once per failed run, and closed when one passes. With a fake gh that
# answers from files and records what it's asked to write.
#
#   scripts/tests/ci-nightly-red.sh
set -euo pipefail

script=$(cd "$(dirname "$0")/.." && pwd)/ci-nightly-red
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

# gh: `run list` prints $tmp/run ("ID CONCLUSION URL", or nothing), `issue
# list` $tmp/issue ("NUMBER STATE", or nothing), `issue view` $tmp/said;
# create, comment and close append to $tmp/calls.
mkdir "$tmp/bin"
cat >"$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
case "$1 $2" in
  "run list") case "$*" in *missing.yml*) echo "HTTP 404: workflow missing.yml not found" >&2; exit 1 ;; esac; cat "$FAKE/run" ;;
  "issue list") cat "$FAKE/issue" ;;
  "issue view") cat "$FAKE/said" ;;
  "issue create"|"issue comment"|"issue close") printf '%s\n' "$*" >>"$FAKE/calls" ;;
  *) echo "fake gh: $*" >&2; exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/gh"
export PATH=$tmp/bin:$PATH FAKE=$tmp GITHUB_REPOSITORY=forge/arugula
unset ARUGULA_RED_MENTION ARUGULA_NIGHTLIES

url=https://github.com/forge/arugula/actions/runs
run() { # NAME: the script on forges-nightly, and what it wrote
  : >"$tmp/calls"
  if "$script" forges-nightly >"$tmp/out" 2>&1; then :; else bad "$1: exit $?: $(cat "$tmp/out")"; fi
}
wrote() { grep -qF -- "$1" "$tmp/calls"; }
quiet() { [ ! -s "$tmp/calls" ]; }

# No run yet: nothing.
: >"$tmp/run"; : >"$tmp/issue"; : >"$tmp/said"
run "no run"
if quiet; then ok "no scheduled run: nothing written"; else bad "no run wrote: $(cat "$tmp/calls")"; fi

# Green with no issue: nothing.
echo "11 success $url/11" >"$tmp/run"
run "green"
if quiet; then ok "a green run with no issue: nothing written"; else bad "green wrote: $(cat "$tmp/calls")"; fi

# Red with no issue: opens it, naming the run.
echo "12 failure $url/12" >"$tmp/run"
run "red"
if wrote "issue create -R forge/arugula --title nightly is red: forges-nightly" && wrote "$url/12"; then ok "a red run opens the issue, naming the run"; else bad "red: $(cat "$tmp/calls")"; fi

# Red again, the same run, issue open and naming it: nothing.
echo "7 OPEN" >"$tmp/issue"
printf '%s\n' "- The scheduled run $url/12 ended failure." >"$tmp/said"
run "same run"
if quiet; then ok "the same red run again: nothing written"; else bad "same run wrote: $(cat "$tmp/calls")"; fi

# A new red run: one comment.
echo "13 failure $url/13" >"$tmp/run"
run "new red run"
if wrote "issue comment 7" && wrote "$url/13" && ! wrote "issue create"; then ok "a new red run comments once"; else bad "new red run: $(cat "$tmp/calls")"; fi

# Cancelled: says nothing.
echo "14 cancelled $url/14" >"$tmp/run"
run "cancelled"
if quiet; then ok "a cancelled run: nothing written"; else bad "cancelled wrote: $(cat "$tmp/calls")"; fi

# Green with the issue open: comments and closes it.
echo "15 success $url/15" >"$tmp/run"
run "green again"
if wrote "issue comment 7" && wrote "issue close 7"; then ok "a green run closes the open issue"; else bad "green again: $(cat "$tmp/calls")"; fi

# Red with a closed issue: opens a new one.
echo "16 timed_out $url/16" >"$tmp/run"
echo "7 CLOSED" >"$tmp/issue"
run "red after closed"
if wrote "issue create" && wrote "$url/16"; then ok "red after a closed issue opens a new one"; else bad "red after closed: $(cat "$tmp/calls")"; fi

# A workflow gh can't find: the others still run, and it exits 1.
echo "17 failure $url/17" >"$tmp/run"
: >"$tmp/issue"
: >"$tmp/calls"
if "$script" missing forges-nightly >"$tmp/out" 2>&1; then bad "a missing workflow exited 0"
elif wrote "$url/17"; then ok "a missing workflow doesn't stop the others, and fails the run"
else bad "missing workflow: $(cat "$tmp/out") / $(cat "$tmp/calls")"; fi

exit $fail
