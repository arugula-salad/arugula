#!/usr/bin/env bash
# scripts/queue-verdict: with a fake chant that reads its answers from a file and
# a fake gh that reads the runs from another, a group passes, waits, re-runs a
# failed run once, and fails when the failure repeats.
#
#   scripts/tests/queue-verdict.sh
set -euo pipefail

script=$(cd "$(dirname "$0")/.." && pwd)/queue-verdict
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

mkdir "$tmp/bin"
# chant: the next exit code from $FAKE/codes, one a line; the last one repeats.
cat >"$tmp/bin/chant" <<'FAKECHANT'
#!/usr/bin/env bash
n=$(cat "$FAKE/n" 2>/dev/null || echo 1)
code=$(sed -n "${n}p" "$FAKE/codes")
[ -n "$code" ] || code=$(tail -1 "$FAKE/codes")
echo $((n + 1)) >"$FAKE/n"
exit "$code"
FAKECHANT
# gh: `api` prints the next block of $FAKE/runs.N; `run rerun` records the call.
cat >"$tmp/bin/gh" <<'FAKEGH'
#!/usr/bin/env bash
case "$1 $2" in
  "run rerun") printf '%s\n' "$*" >>"$FAKE/calls" ;;
  api*)
    n=$(cat "$FAKE/m" 2>/dev/null || echo 1)
    f=$FAKE/runs.$n
    [ -f "$f" ] || f=$(find "$FAKE" -name 'runs.*' | sort | tail -1)
    echo $((n + 1)) >"$FAKE/m"
    cat "$f"
    ;;
  *) echo "fake gh: $*" >&2; exit 1 ;;
esac
FAKEGH
chmod +x "$tmp/bin/chant" "$tmp/bin/gh"
export PATH=$tmp/bin:$PATH FAKE=$tmp ARUGULA_VERDICT_CHANT=$tmp/bin/chant ARUGULA_VERDICT_POLL=0 GITHUB_REPOSITORY=forge/arugula GITHUB_SHA=abc
unset GITHUB_STEP_SUMMARY ARUGULA_VERDICT_MINUTES

# A scenario: the codes chant answers, then the runs gh shows at each look.
scenario() {
  rm -f "$tmp"/runs.* "$tmp/n" "$tmp/m" "$tmp/calls"
  : >"$tmp/calls"
  # shellcheck disable=SC2086 # the codes are words
  printf '%s\n' $1 >"$tmp/codes"
  shift
  local i=1 r
  for r in "$@"; do
    printf '%b' "$r" >"$tmp/runs.$i"
    i=$((i + 1))
  done
  [ $i -gt 1 ] || : >"$tmp/runs.1"
}
want() { # WHAT WANT_EXIT
  local code=0
  timeout 20 "$script" >"$tmp/out" 2>&1 || code=$?
  if [ "$code" = "$2" ]; then ok "$1"; else bad "$1: exit $code: $(cat "$tmp/out")"; fi
}
reran() { # WHAT COUNT
  local n
  n=$(grep -c 'run rerun' "$tmp/calls" || true)
  if [ "$n" = "$2" ]; then ok "$1"; else bad "$1: re-ran $n times: $(cat "$tmp/calls")"; fi
}
T=$'\\t'

scenario "0"
want "every phase passed: passes" 0
reran "and re-runs nothing" 0

scenario "3 3 0"
want "still waiting, then passes" 0

# A flake: a job fails, the run finishes, it's re-run once, and then it passes.
scenario "1 1 3 0" "7${T}1${T}completed${T}failure${T}check\n" "7${T}2${T}in_progress${T}${T}check\n"
want "a flake: passes after its re-run" 0
reran "the failed run is re-run once" 1

# Red while the rest of the run still goes: it waits for the run to finish.
scenario "1 1 0" "7${T}1${T}in_progress${T}${T}check\n" "7${T}1${T}completed${T}failure${T}check\n"
want "red while the run goes: waits, then re-runs" 0
reran "once the run has finished" 1

# It repeats.
scenario "1 1" "7${T}1${T}completed${T}failure${T}check\n" "7${T}2${T}completed${T}failure${T}check\n"
want "the failure repeats: fails" 1
reran "having re-run it once" 1

# GitHub hasn't started the re-run: not asked twice.
scenario "1 1 1 0" "7${T}1${T}completed${T}failure${T}check\n" "7${T}1${T}completed${T}failure${T}check\n" "7${T}2${T}in_progress${T}${T}check\n"
want "a re-run GitHub hasn't started: waits" 0
reran "without asking twice" 1

# chant errors (GitHub's API answering 5xx) with every run still fine: looks again.
scenario "1 1 0" "7${T}1${T}completed${T}success${T}check\n"
want "an API error with nothing failed: looks again, then passes" 0
reran "and re-runs nothing" 0

# An API error while the runs still go.
scenario "1 3 0" "7${T}1${T}in_progress${T}${T}check\n"
want "an API error mid-run: keeps waiting" 0

# Red with nothing to re-run, for good: a cancelled run, say.
scenario "1" "7${T}1${T}completed${T}cancelled${T}check\n"
if ARUGULA_VERDICT_ERRORS=3 timeout 20 "$script" >"$tmp/out" 2>&1; then bad "nothing to re-run should fail in the end"; else ok "nothing to re-run: fails after a few looks"; fi
reran "and re-runs nothing" 0

# Never decided.
scenario "3"
if ARUGULA_VERDICT_MINUTES=0 timeout 20 "$script" >"$tmp/out" 2>&1; then bad "out of time should fail"; else ok "out of time: fails"; fi

exit $fail
