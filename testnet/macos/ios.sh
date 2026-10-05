#!/usr/bin/env bash
#
# Safari in the iOS Simulator (#257, safari:useSimulator), in a fresh clone
# of the Xcode base VM (`vm.sh base --xcode`): web/safari's key probe (#94)
# and presigned invite (#137), and, when asked for, the S27 spike's script
# (safari/safari.ts --ios, from the archive/spikes branch). safaridriver
# runs in the VM and drives Safari in a booted iPhone Simulator, which
# shares the VM's network: the servers forwarded to the VM's loopback and
# its /etc/hosts reach the Simulator too. One VM and one Simulator boot.
#
#   testnet/macos/ios.sh [probe|invite|s27 ...]   probe and invite by default
#   KEEP=1 ...                                    leave the clone running
#
# Parked (#257): on Xcode 27's iOS 27 runtime the Simulator boots and
# safaridriver answers, but refuses every session ("Could not find any
# session hosts"). It needs the Xcode 26 base (vm.sh's default image) and
# hasn't passed yet.
#
# Needs `cargo build -p arugula-control` and web/dist (`just macos ios`
# does both). s27 needs ARUGULA_S27_DIR: spikes/s27-blocks in a worktree of
# archive/spikes, built (`pnpm install`, `cargo build --release`, `node
# build.mjs`, `target/release/s27 cert .run/cert`). ARUGULA_IOS_DEVICE
# names the Simulator's device type (default: the newest iPhone the VM
# has). SAFARI_DRIVER_PORT (default 7744) is the local end of safaridriver;
# S27_SAFARI_PORT (default 7753) is the spike's control on both ends.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
SPIKE="${ARUGULA_S27_DIR:-}"
V="$HERE/vm.sh"
VM="${ARUGULA_MACOS_VM:-arugula-macos-ios}"
PORT="${SAFARI_DRIVER_PORT:-7744}"
CPORT="${S27_SAFARI_PORT:-7753}"
DEVICE="${ARUGULA_IOS_DEVICE:-}"

# shellcheck source=testnet/macos/need-tart.sh
. "$HERE/need-tart.sh"

claims="${*:-probe invite}"
for c in $claims; do case $c in probe | invite | s27) ;;
  *) echo "unknown claim $c (probe invite s27)" >&2; exit 2 ;; esac; done
want() { case " $claims " in *" $1 "*) return 0 ;; esac; return 1; }

[ -f "$ROOT/target/debug/arugula-control" ] && [ -f "$ROOT/web/dist/key-probe.html" ] ||
  { echo "build control and the web app first: \`just macos ios\` does" >&2; exit 2; }
if want s27; then
  [ -n "$SPIKE" ] || { echo "s27 needs ARUGULA_S27_DIR: spikes/s27-blocks in a worktree of archive/spikes (see the top of this script)" >&2; exit 2; }
  [ -f "$SPIKE/.run/cert/cert.pem" ] && [ -x "$SPIKE/target/release/s27" ] ||
    { echo "build the spike in $SPIKE first (see the top of this script)" >&2; exit 2; }
fi

"$V" down "$VM" >/dev/null
"$V" up "$VM" --xcode >/dev/null
fwd=""
udid=""
cleanup() {
  [ -z "$fwd" ] || kill "$fwd" 2>/dev/null || true
  [ -n "${KEEP:-}" ] || "$V" down "$VM" >/dev/null
}
trap cleanup EXIT

# The spike's CA goes into the Mac's keychain and, once the Simulator is
# booted, into the Simulator's; its block names point at loopback.
! want s27 || [ ! -f "$SPIKE/.run/cert/cert.pem" ] || "$V" push "$VM" "$SPIKE/.run/cert/cert.pem" /tmp/s27-cert.pem

# Picks an iPhone (the newest runtime's, or ARUGULA_IOS_DEVICE), making
# one if the image has none, boots it and prints its UDID last. The
# runtime is downloaded only if the image has no iOS one at all.
udid=$("$V" ssh "$VM" "DEVICE='$DEVICE' bash -s" <<'EOF' | tail -1
set -euo pipefail
xcodebuild -version >&2
sudo safaridriver --enable
if [ -f /tmp/s27-cert.pem ]; then
  sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain /tmp/s27-cert.pem
  echo '127.0.0.1 control.test b-s27safari1.blocks.test b-s27safari2.blocks.test' | sudo tee -a /etc/hosts >/dev/null
  sudo dscacheutil -flushcache
fi
newest() { xcrun simctl list runtimes iOS | awk '/^iOS / { r = $NF } END { print r }'; }
runtime=$(newest)
if [ -z "$runtime" ]; then
  echo "no iOS runtime in the image: downloading one" >&2
  xcodebuild -downloadPlatform iOS >&2
  runtime=$(newest)
fi
# `-- iOS 26.0 --` heads each runtime's devices; the last heading is the
# newest runtime.
pick() {
  xcrun simctl list devices available | awk -v want="$DEVICE" '
    /^-- iOS / { ios = 1; next }
    /^-- / { ios = 0; next }
    ios && /iPhone/ && match($0, / \([0-9A-F-]+\)/) {
      id = substr($0, RSTART + 2, RLENGTH - 3)
      name = substr($0, 1, RSTART - 1); sub(/^ +/, "", name)
      if (want == "" || name == want) last = id
    }
    END { if (last != "") print last }'
}
udid=$(pick)
if [ -z "$udid" ]; then
  type=${DEVICE:-$(xcrun simctl list devicetypes | awk '/^iPhone/ { t = $0 } END { sub(/ \(com\.apple.*$/, "", t); print t }')}
  udid=$(xcrun simctl create arugula-iphone "$type" "$runtime")
fi
xcrun simctl bootstatus "$udid" -b >&2
if [ -f /tmp/s27-cert.pem ]; then xcrun simctl keychain "$udid" add-root-cert /tmp/s27-cert.pem; fi
xcrun simctl list devices | grep "$udid" >&2
(nohup safaridriver --port 4444 >/tmp/safaridriver.log 2>&1 &)
sleep 1
echo "$udid"
EOF
)
[[ "$udid" =~ ^[0-9A-F-]{36}$ ]] || { echo "no booted iPhone Simulator in the VM (got: $udid)" >&2; exit 1; }

ssh_cmd=$("$V" sshcmd "$VM")
# shellcheck disable=SC2086 # a command line, split on purpose
$ssh_cmd -N -o ExitOnForwardFailure=yes -L "127.0.0.1:$PORT:127.0.0.1:4444" -R "127.0.0.1:$CPORT:127.0.0.1:$CPORT" &
fwd=$!
for _ in $(seq 1 50); do curl -sf "http://127.0.0.1:$PORT/status" >/dev/null && break; sleep 0.2; done
curl -sf "http://127.0.0.1:$PORT/status" >/dev/null || { echo "safaridriver in the VM never answered" >&2; exit 1; }

export SAFARIDRIVER_URL="http://127.0.0.1:$PORT" SAFARI_DEVICE_UDID="$udid" SAFARI_IOS=1
failed=
pass() { echo "[macos ios $1] ok${2:+: $2}"; }
fail() { echo "[macos ios $1] FAIL: $2" >&2; failed=1; }

# The spec's two tests, picked by title.
web=""
re=""
want probe && web="probe" re="the key probe"
want invite && web="${web:+$web }invite" re="${re:+$re|}a presigned invite"
if [ -n "$web" ]; then
  if (cd "$ROOT/web" && SAFARI_TUNNEL="$ssh_cmd" pnpm exec playwright test -c safari.config.ts -g "$re"); then
    for c in $web; do pass "$c"; done
  else
    for c in $web; do fail "$c" "web/safari/safari.spec.ts in the iOS Simulator (Playwright's report above)"; done
  fi
fi

if want s27; then
  if (cd "$SPIKE" && S27_SAFARI_PORT="$CPORT" node safari/safari.ts --ios); then pass s27 "$SPIKE/.run/verdict-safari-ios-simulator.json"
  else fail s27 "safari/safari.ts --ios (its verdict above, and in $SPIKE/.run/verdict-safari-ios-simulator.json)"; fi
fi

[ -z "$failed" ]
