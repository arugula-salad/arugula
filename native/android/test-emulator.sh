#!/bin/bash
# The core's instrumented tests (core/src/androidTest) on an Android device.
#
#   native/android/test-emulator.sh                        the terminal and TLS
#   native/android/test-emulator.sh URL MACHINE ROOM HANDOFF
#                                                          and the flow against
#                                                          the control at URL
#
# With URL, the test signs in, lists projects, posts in ROOM, and types into a
# new pane on MACHINE through the relay. Something else allows the sign-in, as
# for the iPhone's test-sim.sh: the ticket is written to HANDOFF/ticket, and
# the address the Allow was sent to is read from HANDOFF/callback. The cloud
# repository's testnet `native-android` claim drives it. A URL on 127.0.0.1 is
# the host's: its port is reversed onto the device.
#
# It uses the device adb already has (ANDROID_SERIAL), else boots the AVD
# arugula-core (setup.sh) headless and shuts it down after. The core is built
# for that device's ABI only.
set -euo pipefail
cd "$(dirname "$0")"
url=${1:-} machine=${2:-} room=${3:-} handoff=${4:-}
[ -z "$url" ] || [ -n "$handoff" ] || { echo "usage: test-emulator.sh [URL MACHINE ROOM HANDOFF]" >&2; exit 2; }
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
adb() { "$ANDROID_HOME/platform-tools/adb" "$@"; }
pkg=io.arugula.core.test
log=$(mktemp -d)

if [ -z "${ANDROID_SERIAL:-}" ]; then
  port=${ARUGULA_EMULATOR_PORT:-5580}
  export ANDROID_SERIAL=emulator-$port
  "$ANDROID_HOME/emulator/emulator" -avd arugula-core -port "$port" -no-window -no-audio -no-boot-anim \
    -no-snapshot -gpu swiftshader_indirect > "$log/emulator.log" 2>&1 &
  emu=$!
  trap 'adb emu kill >/dev/null 2>&1 || kill $emu 2>/dev/null || true; rm -rf "$log"' EXIT
  end=$((SECONDS + 300))
  until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do
    if [ $SECONDS -gt $end ] || ! kill -0 $emu 2>/dev/null; then
      echo "test-emulator.sh: the emulator didn't boot" >&2; tail -20 "$log/emulator.log" >&2; exit 1
    fi
    sleep 2
  done
else
  trap 'rm -rf "$log"' EXIT
fi
abi=$(adb shell getprop ro.product.cpu.abi | tr -d '\r')
echo "test-emulator.sh: $ANDROID_SERIAL ($abi)"

./gradlew -q :core:assembleDebugAndroidTest -Parugula.abis="$abi"
adb install -r -t core/build/outputs/apk/androidTest/debug/core-debug-androidTest.apk >/dev/null
adb shell run-as $pkg rm -rf files/handoff

args=()
if [ -n "$url" ]; then
  case "$url" in
    http://127.0.0.1:* | http://localhost:*) p=${url##*:}; adb reverse "tcp:${p%%/*}" "tcp:${p%%/*}" >/dev/null ;;
  esac
  args=(-e control "$url" -e machine "$machine" -e room "$room")
  rm -rf "$handoff"; mkdir -p "$handoff"
fi
# No network on CI's nightly beyond the internet: TLS against the real one.
[ -z "${ARUGULA_NO_TLS_CHECK:-}" ] || args+=(-e tls off)

adb shell am instrument -w -r "${args[@]}" $pkg/androidx.test.runner.AndroidJUnitRunner > "$log/run.out" 2>&1 &
run=$!
# The ticket out of the app's files, the callback in.
if [ -n "$url" ]; then
  pushed=""
  while kill -0 $run 2>/dev/null; do
    if [ ! -s "$handoff/ticket" ]; then
      t=$(adb shell run-as $pkg cat files/handoff/ticket 2>/dev/null | tr -d '\r' || true)
      [ -z "$t" ] || printf '%s\n' "$t" > "$handoff/ticket"
    elif [ -z "$pushed" ] && [ -s "$handoff/callback" ]; then
      adb shell "run-as $pkg sh -c 'cat > files/handoff/callback'" < "$handoff/callback" && pushed=1
    fi
    sleep 1
  done
fi
wait $run || true
grep -E '^(INSTRUMENTATION_STATUS: (test|stack)=|OK \(|FAILURES|Tests run)' "$log/run.out" | grep -v 'stack=$' | tail -40
[ -z "$handoff" ] || cp "$log/run.out" "$handoff/instrument.out"
grep -q '^OK (' "$log/run.out"
