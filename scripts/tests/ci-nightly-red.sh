#!/usr/bin/env bash
# scripts/ci-nightly-red: one "nightly is red: WORKFLOW" issue per
# scheduled workflow, opened when its newest scheduled run fails, commented
# on once per failed run, and closed when one passes or a newer manual
# run on the same or a descendant commit passes. With a fake gh that
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

# A small real history lets recovery check commit ancestry, including a
# manual run against an old commit or an unrelated branch.
git init -q "$tmp/repo"
git -C "$tmp/repo" config user.name Test
git -C "$tmp/repo" config user.email test@example.invalid
git -C "$tmp/repo" commit -q --allow-empty -m old
old=$(git -C "$tmp/repo" rev-parse HEAD)
git -C "$tmp/repo" commit -q --allow-empty -m failed
failed=$(git -C "$tmp/repo" rev-parse HEAD)
git -C "$tmp/repo" commit -q --allow-empty -m fixed
fixed=$(git -C "$tmp/repo" rev-parse HEAD)
git -C "$tmp/repo" checkout -q --orphan unrelated
git -C "$tmp/repo" commit -q --allow-empty -m unrelated
unrelated=$(git -C "$tmp/repo" rev-parse HEAD)

# gh: scheduled `run list` prints $tmp/run ("ID CONCLUSION URL CREATED SHA"),
# manual `run list` $tmp/manual ("CONCLUSION URL CREATED SHA"), `issue list`
# $tmp/issue ("NUMBER STATE"), `issue view` $tmp/said; create, comment and
# close append to $tmp/calls. Reject a query not scoped to main.
mkdir "$tmp/bin"
cat >"$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
case "$1 $2" in
  "run list")
    case "$*" in *missing.yml*) echo "HTTP 404: workflow missing.yml not found" >&2; exit 1 ;; esac
    case " $* " in *" -b main "*) ;; *) echo "run list must filter main" >&2; exit 1 ;; esac
    case " $* " in
      *" -e schedule "*) cat "$FAKE/run" ;;
      *" -e workflow_dispatch "*)
        if [ -e "$FAKE/manual-error" ]; then echo "HTTP 503" >&2; exit 1; fi
        cat "$FAKE/manual" ;;
      *) echo "unexpected run event: $*" >&2; exit 1 ;;
    esac ;;
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
  if (cd "$tmp/repo" && "$script" forges-nightly) >"$tmp/out" 2>&1; then :; else bad "$1: exit $?: $(cat "$tmp/out")"; fi
}
wrote() { grep -qF -- "$1" "$tmp/calls"; }
quiet() { [ ! -s "$tmp/calls" ]; }

# No run yet: nothing.
: >"$tmp/run"; : >"$tmp/manual"; : >"$tmp/issue"; : >"$tmp/said"
run "no run"
if quiet; then ok "no scheduled run: nothing written"; else bad "no run wrote: $(cat "$tmp/calls")"; fi

# Green with no issue: nothing.
echo "11 success $url/11 2026-10-26T00:00:00Z $failed" >"$tmp/run"
run "green"
if quiet; then ok "a green run with no issue: nothing written"; else bad "green wrote: $(cat "$tmp/calls")"; fi

# Red with no issue: opens it, naming the run.
echo "12 failure $url/12 2026-10-26T00:00:00Z $failed" >"$tmp/run"
run "red"
if wrote "issue create -R forge/arugula --title nightly is red: forges-nightly" && wrote "$url/12"; then ok "a red run opens the issue, naming the run"; else bad "red: $(cat "$tmp/calls")"; fi

# Red again, the same run, issue open and naming it: nothing.
echo "7 OPEN" >"$tmp/issue"
printf '%s\n' "- The scheduled run $url/12 ended failure." >"$tmp/said"
run "same run"
if quiet; then ok "the same red run again: nothing written"; else bad "same run wrote: $(cat "$tmp/calls")"; fi

# A new red run: one comment.
echo "13 failure $url/13 2026-10-26T00:00:00Z $failed" >"$tmp/run"
run "new red run"
if wrote "issue comment 7" && wrote "$url/13" && ! wrote "issue create"; then ok "a new red run comments once"; else bad "new red run: $(cat "$tmp/calls")"; fi

# Cancelled: says nothing.
echo "14 cancelled $url/14 2026-10-26T00:00:00Z $failed" >"$tmp/run"
run "cancelled"
if quiet; then ok "a cancelled run: nothing written"; else bad "cancelled wrote: $(cat "$tmp/calls")"; fi

# Green with the issue open: comments and closes it.
echo "15 success $url/15 2026-10-26T00:00:00Z $failed" >"$tmp/run"
run "green again"
if wrote "issue comment 7" && wrote "issue close 7"; then ok "a green run closes the open issue"; else bad "green again: $(cat "$tmp/calls")"; fi

# Red with a closed issue: opens a new one.
echo "16 timed_out $url/16 2026-10-26T00:00:00Z $failed" >"$tmp/run"
echo "7 CLOSED" >"$tmp/issue"
run "red after closed"
if wrote "issue create" && wrote "$url/16"; then ok "red after a closed issue opens a new one"; else bad "red after closed: $(cat "$tmp/calls")"; fi

# A newer successful manual run validates the same commit or its fix.
echo "7 OPEN" >"$tmp/issue"
for sha in "$failed" "$fixed"; do
  echo "success $url/18 2026-10-28T00:00:00Z $sha" >"$tmp/manual"
  run "manual recovery"
  if wrote "issue close 7" && wrote "$url/18" && ! wrote "$url/16"; then ok "newer manual success on $sha closes with its run URL"; else bad "manual recovery: $(cat "$tmp/calls")"; fi
done

# Don't reopen the old alert on the following tick.
echo "7 CLOSED" >"$tmp/issue"
run "manual recovery already closed"
if quiet; then ok "manual recovery doesn't reopen an old failure"; else bad "manual recovery reopened: $(cat "$tmp/calls")"; fi

# An old success can't clear a newer scheduled failure.
echo "7 OPEN" >"$tmp/issue"
echo "success $url/18 2026-10-25T00:00:00Z $fixed" >"$tmp/manual"
run "older manual run"
if wrote "$url/16" && ! wrote "issue close"; then ok "an older manual run doesn't clear the alert"; else bad "older manual run: $(cat "$tmp/calls")"; fi

# Nor can a newly started run of older or unrelated code.
for sha in "$old" "$unrelated"; do
  echo "success $url/18 2026-10-28T00:00:00Z $sha" >"$tmp/manual"
  run "old or unrelated manual commit"
  if wrote "$url/16" && ! wrote "issue close"; then ok "manual success on $sha doesn't clear newer code's failure"; else bad "manual commit: $(cat "$tmp/calls")"; fi
done

# Only a successful completed manual run counts as recovery.
for conclusion in failure cancelled skipped; do
  echo "$conclusion $url/18 2026-10-28T00:00:00Z $fixed" >"$tmp/manual"
  run "manual $conclusion"
  if wrote "$url/16" && ! wrote "issue close"; then ok "manual $conclusion doesn't clear the alert"; else bad "manual $conclusion: $(cat "$tmp/calls")"; fi
done

# A failed recovery lookup still reports the scheduled failure, and exits 1.
touch "$tmp/manual-error"
: >"$tmp/calls"
if (cd "$tmp/repo" && "$script" forges-nightly) >"$tmp/out" 2>&1; then bad "a failed manual lookup exited 0"
elif wrote "$url/16" && ! wrote "issue close"; then ok "a failed manual lookup preserves the alert and fails the run"
else bad "manual lookup: $(cat "$tmp/out") / $(cat "$tmp/calls")"; fi
rm "$tmp/manual-error"
: >"$tmp/manual"

# A workflow gh can't find: the others still run, and it exits 1.
echo "17 failure $url/17 2026-10-26T00:00:00Z $failed" >"$tmp/run"
: >"$tmp/issue"
: >"$tmp/calls"
if (cd "$tmp/repo" && "$script" missing forges-nightly) >"$tmp/out" 2>&1; then bad "a missing workflow exited 0"
elif wrote "$url/17"; then ok "a missing workflow doesn't stop the others, and fails the run"
else bad "missing workflow: $(cat "$tmp/out") / $(cat "$tmp/calls")"; fi

exit $fail
