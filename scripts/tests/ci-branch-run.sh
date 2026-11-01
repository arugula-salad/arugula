#!/usr/bin/env bash
# scripts/ci-branch-run (#304): `touches` picks out the files that make a
# pull request need a check.yml run on its branch; `gate` passes a pull
# request that changes none of them or whose head has a passing check.yml
# run, waits for one in progress, and fails with what to do otherwise;
# `rerun` re-runs binaries.yml's run once it has finished, only when its
# ci-branch-run job failed. With a fake gh that answers from canned JSON
# (through the real jq) and records what it's asked to re-run.
#
#   scripts/tests/ci-branch-run.sh
set -euo pipefail

script=$(cd "$(dirname "$0")/.." && pwd)/ci-branch-run
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

# touches: each line of IN counts or doesn't.
counts() {
  local f
  for f in "$@"; do
    if printf '%s\n' "$f" | "$script" touches >/dev/null; then ok "counts: $f"; else bad "should count: $f"; fi
  done
}
not_counts() {
  local f
  for f in "$@"; do
    if printf '%s\n' "$f" | "$script" touches >/dev/null; then bad "shouldn't count: $f"; else ok "doesn't count: $f"; fi
  done
}
counts .github/workflows/check.yml .github/actions/setup/action.yml .github/CODEOWNERS justfile \
  chant.workspace.json .config/nextest.toml scripts/ci-env scripts/ci-branch-run scripts/app-changed \
  scripts/suite-changed scripts/changed
not_counts docs/development.md AGENTS.md .config/quarantine.toml scripts/check-binaries scripts/release \
  scripts/tests/ci-red.sh web/justfile crates/daemon/src/lib.rs scripts/changed-not ''
got=$(printf '%s\n' docs/a.md justfile crates/x.rs scripts/ci-red | "$script" touches)
if [ "$got" = $'justfile\nscripts/ci-red' ]; then ok "touches prints the ones that count"; else bad "touches printed: $got"; fi

# gh: `api URL [--paginate] --jq EXPR` answers from $FAKE: a pull
# request's files from files.json, check.yml's runs from check.N.json (N
# counts the calls, the last one repeating), binaries.yml's runs from
# binaries.N.json likewise, a run's jobs from jobs.json. `run rerun`
# appends to calls.
mkdir "$tmp/bin"
cat >"$tmp/bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
# The newest of NAME.1.json, NAME.2.json... that this call reaches.
seq_file() {
  local n=0 i
  [ -f "$FAKE/$1.n" ] && n=$(cat "$FAKE/$1.n")
  n=$((n + 1))
  echo "$n" >"$FAKE/$1.n"
  for ((i = n; i > 0; i--)); do
    [ -f "$FAKE/$1.$i.json" ] && { echo "$FAKE/$1.$i.json"; return; }
  done
  echo "$FAKE/none.json"
}
if [ "$1" = run ] && [ "$2" = rerun ]; then
  printf '%s\n' "$*" >>"$FAKE/calls"
  exit 0
fi
[ "$1" = api ] || { echo "fake gh: $*" >&2; exit 1; }
url='' jq_expr=.
shift
while [ $# -gt 0 ]; do
  case "$1" in --jq) jq_expr=$2; shift 2 ;; --*) shift ;; *) url=$1; shift ;; esac
done
case "$url" in
  repos/forge/arugula/pulls/7/files) f=$FAKE/files.json ;;
  repos/forge/arugula/actions/workflows/check.yml/runs\?head_sha=abc123*) f=$(seq_file check) ;;
  repos/forge/arugula/actions/workflows/binaries.yml/runs\?head_sha=abc123\&event=pull_request*) f=$(seq_file binaries) ;;
  repos/forge/arugula/actions/runs/55/jobs*) f=$FAKE/jobs.json ;;
  *) echo "fake gh: no $url" >&2; exit 1 ;;
esac
jq -r "$jq_expr" "$f"
EOF
chmod +x "$tmp/bin/gh"
export PATH=$tmp/bin:$PATH GH_REPO=forge/arugula ARUGULA_BRANCH_RUN_POLL=0

# reset: a new $FAKE, with no runs and no calls (the pull request's files
# carried over).
reset() {
  local old=${FAKE:-}
  FAKE=$(mktemp -d "$tmp/fake.XXXXXX")
  export FAKE
  echo '{"workflow_runs": []}' >"$FAKE/none.json"
  : >"$FAKE/calls"
  [ -z "$old" ] || [ ! -f "$old/files.json" ] || cp "$old/files.json" "$FAKE/"
}
files() { # NAME[:OLD]...
  local a=() f
  for f in "$@"; do
    case "$f" in
      *:*) a+=("{\"filename\": \"${f%%:*}\", \"previous_filename\": \"${f#*:}\"}") ;;
      *) a+=("{\"filename\": \"$f\"}") ;;
    esac
  done
  (IFS=,; echo "[${a[*]}]") >"$FAKE/files.json"
}
runs() { # FILE STATUS:CONCLUSION...
  local out=$1 a=() r
  shift
  for r in "$@"; do
    a+=("{\"id\": 55, \"status\": \"${r%%:*}\", \"conclusion\": $([ "${r#*:}" = - ] && echo null || echo "\"${r#*:}\""), \"html_url\": \"https://example.invalid/runs/${r//:/-}\"}")
  done
  (IFS=,; echo "{\"workflow_runs\": [${a[*]}]}") >"$FAKE/$out"
}
# gate WANT NAME [ENV...]: ci-branch-run gate's exit status is WANT; its
# output is in $tmp/out.
gate() {
  local want=$1 name=$2 got=0
  shift 2
  env "$@" "$script" gate 7 abc123 >"$tmp/out" 2>&1 || got=$?
  if [ "$got" = "$want" ]; then ok "gate: $name: $(tail -1 "$tmp/out")"; else bad "gate: $name: exit $got, wanted $want: $(cat "$tmp/out")"; fi
}
says() { # NAME TEXT: the last command's output has TEXT
  if grep -qF -- "$2" "$tmp/out"; then ok "$1"; else bad "$1: no \"$2\" in: $(cat "$tmp/out")"; fi
}

reset
files docs/development.md crates/daemon/src/lib.rs
gate 0 "no CI file"
says "says no run is needed" "no branch run needed"

files docs/development.md .github/workflows/binaries.yml
gate 1 "CI changed, no run"
says "says how to get a run" "full-check label"
says "and for a fork" "A fork's branch"
says "as an error annotation" "::error::"

files scripts/ci-old-name:scripts/elsewhere docs/x.md
gate 1 "a rename's new name counts"
files scripts/elsewhere:scripts/ci-old docs/x.md
gate 1 "a rename's old name counts"

files justfile
runs check.1.json completed:success
gate 0 "a passing run"
says "names the run" "https://example.invalid/runs/completed-success"

reset
runs check.1.json completed:failure
gate 1 "a failed run"
says "says it failed" "check.yml failed on abc123"

reset
runs check.1.json completed:failure completed:success
gate 0 "a failed run and a passing one"

reset
runs check.1.json in_progress:-
runs check.3.json completed:success
gate 0 "a run in progress that passes"
says "said it waited" "waiting for it"

reset
runs check.1.json queued:-
runs check.2.json completed:failure
gate 1 "a queued run that fails"
says "says it failed" "check.yml failed"

reset
runs check.1.json in_progress:-
gate 1 "a run still going when the wait ends" ARUGULA_BRANCH_RUN_WAIT=0
says "says to re-run" "still running"

# rerun WANT_CALLS NAME [ENV...]: ci-branch-run rerun exits 0 and asks gh
# to re-run WANT_CALLS ("" for nothing).
rerun() {
  local want=$1 name=$2 got=0
  shift 2
  : >"$FAKE/calls"
  env "$@" "$script" rerun abc123 >"$tmp/out" 2>&1 || got=$?
  if [ "$got" = 0 ] && [ "$(cat "$FAKE/calls")" = "$want" ]; then ok "rerun: $name: $(tail -1 "$tmp/out")"
  else bad "rerun: $name: exit $got, re-ran \"$(cat "$FAKE/calls")\", wanted \"$want\": $(cat "$tmp/out")"; fi
}
jobs() { # NAME:CONCLUSION...
  local a=() j
  for j in "$@"; do a+=("{\"id\": 9, \"name\": \"${j%%:*}\", \"conclusion\": \"${j#*:}\"}"); done
  (IFS=,; echo "{\"jobs\": [${a[*]}]}") >"$FAKE/jobs.json"
}

reset
runs check.1.json queued:-
runs binaries.1.json completed:failure
jobs binaries:success quick:success ci-branch-run:failure queue:skipped
rerun "run rerun 55 --failed" "the gate failed"

reset
runs check.2.json queued:-
runs binaries.1.json completed:failure
jobs ci-branch-run:failure
rerun "run rerun 55 --failed" "the dispatched run shows up on the second look"

reset
runs check.1.json queued:-
runs binaries.1.json in_progress:-
runs binaries.3.json completed:failure
jobs ci-branch-run:failure
rerun "run rerun 55 --failed" "binaries.yml finishes, then the gate's re-run"
says "said it waited" "waiting for it to finish"

reset
runs check.1.json queued:-
runs binaries.1.json completed:failure
jobs quick:failure ci-branch-run:success
rerun "" "only quick failed"

reset
runs check.1.json queued:-
rerun "" "no binaries.yml run"

reset
rerun "" "no check.yml run shows up" ARUGULA_BRANCH_RUN_WAIT=0
says "says so" "no check.yml run"

reset
runs check.1.json queued:-
runs binaries.1.json in_progress:-
rerun "" "binaries.yml still going when the wait ends" ARUGULA_BRANCH_RUN_WAIT=0
says "says to re-run by hand" "by hand"

exit $fail
