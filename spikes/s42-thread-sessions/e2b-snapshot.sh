#!/usr/bin/env bash
# S42 q3 (prototype): snapshot a set-up box on E2B, wipe it, snapshot it
# again, and check what a box made from each carries. See e2b-snapshot.mjs
# for the steps. Every sandbox and snapshot it makes is deleted before it
# exits, and it lists what's left to show it.
#
#   E2B_API_KEY=… spikes/s42-thread-sessions/e2b-snapshot.sh [OUT]
#
# - E2B_API_KEY: read from the environment, never printed.
# - S42_ARUGULAD: a static arugulad (`just static`) to run in the wiped
#   box, to see it make keys of its own in the next one. It's pointed at
#   127.0.0.1:9, so it never enrolls anywhere. Optional.
# - S42_BACKEND=loam: the same on loam (its key as E2B_API_KEY), with
#   loam's checkpoints and forks.
# - OUT: where the log goes (default results/e2b-snapshot.txt, or
#   results/loam-snapshot.txt).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
[ -n "${E2B_API_KEY:-}" ] || { echo "E2B_API_KEY is not set" >&2; exit 1; }
default=e2b-snapshot.txt
[ "${S42_BACKEND:-}" = loam ] && default=loam-snapshot.txt
out=${1:-$here/results/$default}
mkdir -p "$(dirname "$out")"
[ -d "$root/packaging/hosted/node_modules/e2b" ] || npm ci --prefix "$root/packaging/hosted" --no-fund --no-audit --silent
set +e
node "$here/e2b-snapshot.mjs" 2>&1 | tee "$out"
code=${PIPESTATUS[0]}
set -e
# The key must not be in what's kept.
if grep -qF -- "$E2B_API_KEY" "$out"; then
  echo "the key got into $out: removed it" >&2
  rm -f "$out"
  exit 1
fi
exit "$code"
