#!/usr/bin/env bash
# The line between the cloud, the daemon and the native clients
# (scripts/crate-boundary.mjs, Arugula #809), on small fake workspaces: it
# passes when they share only the shared crates, and fails, naming the edge,
# on each kind of crossing.
#
#   scripts/tests/crate-boundary.sh
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() { printf 'FAIL  %s\n' "$*"; fail=1; }
ok() { printf 'ok    %s\n' "$*"; }

# crate DIR NAME [DEP=PATH[:dev]...]: a library crate at DIR.
crate() {
  local dir=$1 name=$2 deps="" dev=""
  shift 2
  for d in "$@"; do
    local n=${d%%=*} p=${d#*=}
    if [[ $p == *:dev ]]; then dev+="$n = { path = \"${p%:dev}\" }"$'\n'; else deps+="$n = { path = \"$p\" }"$'\n'; fi
  done
  mkdir -p "$dir/src"
  : >"$dir/src/lib.rs"
  printf '[package]\nname = "%s"\nversion = "0.0.0"\nedition = "2021"\n\n[dependencies]\n%s\n[dev-dependencies]\n%s' "$name" "$deps" "$dev" >"$dir/Cargo.toml"
}

# The open repo: the root workspace (two shared crates, a daemon-only one,
# a testkit), native/core and the desktop app, each a workspace of its own.
open=$tmp/cloud/open
mkdir -p "$open/scripts"
cp "$here"/crate-boundary.mjs "$open/scripts/"
printf '[workspace]\nresolver = "2"\nmembers = ["crates/*"]\nexclude = ["crates/desktop"]\n' >"$open/Cargo.toml"
for s in control-wire core e2e forge-github prices; do crate "$open/crates/$s" "arugula-$s"; done
crate "$open/crates/proto" arugula-proto arugula-core=../core
crate "$open/crates/testkit" arugula-testkit arugula-proto=../proto
crate "$open/crates/daemon" arugulad-base arugula-proto=../proto arugula-testkit=../testkit:dev
crate "$open/crates/desktop" arugula-desktop arugula-proto=../proto
printf '\n[workspace]\n' >>"$open/crates/desktop/Cargo.toml"
crate "$open/native/core" arugula-native-core arugula-proto=../../crates/proto arugula-e2e=../../crates/e2e
printf '\n[workspace]\n' >>"$open/native/core/Cargo.toml"

# The cloud: a workspace beside open/, on two shared crates.
cloud=$tmp/cloud
printf '[workspace]\nresolver = "2"\nmembers = ["crates/*"]\nexclude = ["open"]\n' >"$cloud/Cargo.toml"
crate "$cloud/crates/common" cloud-common arugula-control-wire=../../open/crates/control-wire
crate "$cloud/crates/platform" cloud-platform cloud-common=../common arugula-forge-github=../../open/crates/forge-github

# expect NAME pass|fail ['TEXT'] [--cloud]: the check's verdict, and TEXT in what it says.
expect() {
  local name=$1 want=$2 text=${3:-} out st=0
  shift 3 || shift $#
  out=$(cd "$tmp" && node "$open/scripts/crate-boundary.mjs" "$@" 2>&1) || st=$?
  if [ "$want" = pass ] && [ "$st" != 0 ]; then bad "$name: failed: $out"; return; fi
  if [ "$want" = fail ] && [ "$st" != 1 ]; then bad "$name: exit $st, not 1: $out"; return; fi
  if [ -n "$text" ] && ! grep -qF -- "$text" <<<"$out"; then bad "$name: doesn't say '$text': $out"; return; fi
  ok "$name"
}
# change FILE FROM TO: one edit, undone by restore.
change() { cp "$1" "$1.orig"; perl -pi -e "s{\\Q$2\\E}{$3}" "$1"; grep -qF -- "$3" "$1" || bad "the test couldn't edit $1"; }
restore() { mv "$1.orig" "$1"; }

expect "the open repo passes when only shared crates cross" pass "share only arugula-control-wire, arugula-core"
expect "the cloud passes on shared crates" pass "the cloud and daemon" --cloud "$cloud"

f=$open/native/core/Cargo.toml
change "$f" 'arugula-e2e = { path = "../../crates/e2e" }' 'arugulad-base = { path = "../../crates/daemon" }'
expect "native/core on a daemon-only crate fails" fail "arugula-native-core (native) -> arugulad-base, a dependency at crates/daemon: arugulad-base is the daemon workspace's, and not shared"
restore "$f"

f=$open/crates/desktop/Cargo.toml
change "$f" '[dev-dependencies]' "[dev-dependencies]
arugula-testkit = { path = \"../testkit\" }"
expect "the desktop app on a daemon-only crate, even for its tests, fails" fail "arugula-desktop (desktop) -> arugula-testkit, a dev-dependency at crates/testkit"
restore "$f"

f=$open/crates/proto/Cargo.toml
change "$f" 'arugula-core = { path = "../core" }' 'arugula-core = { path = "../core" }
arugulad-base = { path = "../daemon" }'
expect "a shared crate on a daemon-only one fails" fail "arugula-proto (daemon) -> arugulad-base, a dependency at crates/daemon: arugula-proto is shared, so the cloud and the clients would build arugulad-base"
restore "$f"

f=$open/crates/proto/Cargo.toml
change "$f" '[dev-dependencies]' '[dev-dependencies]
arugula-testkit = { path = "../testkit" }'
expect "a shared crate's tests may use a daemon-only one" pass
restore "$f"

f=$open/crates/daemon/Cargo.toml
change "$f" '[dependencies]' '[dependencies]
cloud-common = { path = "../../../crates/common" }'
expect "an open crate on the cloud's fails" fail "arugulad-base (daemon) -> cloud-common, a dependency at ../crates/common: it's outside this repository"
restore "$f"

f=$open/crates/daemon/Cargo.toml
change "$f" '[dependencies]' '[dependencies]
arugula-native-core = { path = "../../native/core" }'
# Cargo refuses this one itself; the check says so.
expect "the daemon on native/core fails" fail "multiple workspace roots"
restore "$f"

f=$cloud/crates/common/Cargo.toml
change "$f" 'arugula-control-wire = { path = "../../open/crates/control-wire" }' 'arugulad-base = { path = "../../open/crates/daemon" }'
expect "the cloud on a daemon-only crate fails" fail "cloud-common (cloud) -> arugulad-base, a dependency at open/crates/daemon: arugulad-base is the open repo's daemon workspace's, and not shared" --cloud "$cloud"
restore "$f"

f=$open/crates/proto/Cargo.toml
change "$f" 'arugula-core = { path = "../core" }' 'arugula-core = { path = "../core" }
arugulad-base = { path = "../daemon" }'
expect "the cloud's check checks the shared crates too" fail "arugula-proto is shared" --cloud "$cloud"
restore "$f"

mkdir -p "$open/spikes/x/src" && : >"$open/spikes/x/src/lib.rs"
printf '[package]\nname = "x"\nversion = "0.0.0"\nedition = "2021"\n\n[workspace]\n' >"$open/spikes/x/Cargo.toml"
expect "a workspace it doesn't know fails" fail "spikes/x/Cargo.toml is a Cargo workspace that scripts/crate-boundary.mjs doesn't know"
rm -rf "$open/spikes"

mv "$open/crates/prices" "$tmp/prices"
expect "a shared crate that's gone fails" fail "arugula-prices is in SHARED but no workspace here has it"
mv "$tmp/prices" "$open/crates/prices"

expect "and passes again" pass

exit $fail
