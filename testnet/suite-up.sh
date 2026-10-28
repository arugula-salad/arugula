#!/usr/bin/env bash
#
# Bring the test stack up for the Rust suite, before nextest starts:
# `just test-run` runs it. The suite's testnet tests (ssh::, reboot:: and
# guest_ssh's jump host, crates/arugulad/tests/integration) each bring the
# stack up if it isn't, but inside a test the image build counts against
# its deadline (nextest's 5 minutes), and on a CI pod's cold Docker that
# build alone (apt-get on every box) took 3-4 minutes: the first test timed
# out, and the next one tripped over the containers the killed `up` left
# half made. Here the build is no test's.
#
#   testnet/suite-up.sh [nextest's --partition ...]
#
# Brings up `control` when the run has guest_ssh's jump host test and the
# static binaries are there (arugula-control), `ssh` when it has only the
# others, and nothing when it has none of them. Never fails the suite: if
# the stack can't come up, the tests try again themselves and fail saying
# why. With no Docker (or ARUGULA_SKIP_DOCKER=1) it does nothing; the
# tests say what that means.
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
log() { echo "[testnet suite-up] $*" >&2; }

[ "${ARUGULA_SKIP_DOCKER:-}" = 1 ] && exit 0
docker info >/dev/null 2>&1 || exit 0

# This run's tests in nextest.toml's `testnet` group. The whole partition
# is listed, as the run makes it (a filter here would partition only what
# it matched).
all="$(cd "$HERE/.." && mise exec -- cargo nextest list --workspace "$@" --message-format oneline 2>/dev/null)" || {
  log "couldn't list the suite's tests; the testnet tests bring the stack up themselves"
  exit 0
}
tests="$(grep -E '^arugulad::integration (ssh::|reboot::|guest_ssh::.*jump_host)' <<<"$all")"
[ -n "$tests" ] || exit 0

profile=ssh
if grep -q 'guest_ssh::' <<<"$tests"; then
  # control mounts the box's binaries, as testnet::require finds them.
  arch="$(docker info --format '{{.Architecture}}')"
  case "$arch" in arm64) arch=aarch64 ;; amd64) arch=x86_64 ;; esac
  bins="${ARUGULA_TESTNET_BINARIES:-${ARUGULA_SSH_BINARIES:-${CARGO_TARGET_DIR:-$HERE/../target}/$arch-unknown-linux-musl/release}}"
  if [ -x "$bins/arugula-control" ]; then
    profile=control
    export ARUGULA_TESTNET_BINARIES="$bins"
  fi
fi
log "the $profile profile, before the suite"
"$HERE/up.sh" "$profile" >/dev/null || log "testnet/up.sh $profile failed; the testnet tests try again themselves"
exit 0
