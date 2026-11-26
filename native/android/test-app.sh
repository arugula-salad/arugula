#!/bin/bash
# The app's UI test (app/src/androidTest) on an Android device, against the
# control at URL, as the iPhone's test-sim.sh:
#
#   native/android/test-app.sh URL MACHINE ROOM HANDOFF
#
# It signs in, goes through the tabs (Home answers the board's question in
# its row, Boards opens the board, the project's menu its issues), posts in
# ROOM from Chat, types into a new terminal on MACHINE, lists Docs and opens
# one if the project has any, and signs out.
# Something else allows the sign-in: the ticket is written to HANDOFF/ticket,
# and the address the Allow was sent to is read from HANDOFF/callback. The
# cloud repository's testnet `native-android` claim drives it. ARUGULA_BOARD
# names a board home should show, and ARUGULA_ASK its row's question to the
# person signing in. ARUGULA_TEAM names a team's project with
# ARUGULA_GRANT_BOARD, a board with a grant the person gave, whose Revoke the
# test presses, and ARUGULA_RUN_BOARD, one with no lead and no grant, whose
# Run this board it presses (Arugula #1456). ARUGULA_TODO is a stand-in
# agent's to-do (its prp_ id, Arugula #1470), marked Done under Needs you with
# ARUGULA_TODO_NOTE. Screenshots land in HANDOFF. A URL on 127.0.0.1 is the
# host's: its port is reversed onto the device.
#
# It uses the device adb already has (ANDROID_SERIAL), else boots the AVD
# arugula-core (setup.sh) headless and shuts it down after. The core is built
# for that device's ABI only.
set -euo pipefail
cd "$(dirname "$0")"
url=${1:?usage: test-app.sh URL MACHINE ROOM HANDOFF} machine=${2:?} room=${3:?} handoff=${4:?}
export ANDROID_HOME=${ANDROID_HOME:-$HOME/Android/Sdk}
export ANDROID_AVD_HOME=${ANDROID_AVD_HOME:-$HOME/.android/avd}
adb() { "$ANDROID_HOME/platform-tools/adb" "$@"; }
pkg=io.arugula.app
log=$(mktemp -d)

if [ -z "${ANDROID_SERIAL:-}" ]; then
  port=${ARUGULA_EMULATOR_PORT:-5580}
  export ANDROID_SERIAL=emulator-$port
  "$ANDROID_HOME/emulator/emulator" -avd arugula-core -port "$port" -read-only -no-window -no-audio -no-boot-anim \
    -no-snapshot -gpu swiftshader_indirect > "$log/emulator.log" 2>&1 &
  emu=$!
  trap 'adb emu kill >/dev/null 2>&1 || kill $emu 2>/dev/null || true; rm -rf "$log"' EXIT
  end=$((SECONDS + 300))
  until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do
    if [ $SECONDS -gt $end ] || ! kill -0 $emu 2>/dev/null; then
      echo "test-app.sh: the emulator didn't boot" >&2; tail -20 "$log/emulator.log" >&2; exit 1
    fi
    sleep 2
  done
else
  trap 'rm -rf "$log"' EXIT
fi
abi=$(adb shell getprop ro.product.cpu.abi | tr -d '\r')
echo "test-app.sh: $ANDROID_SERIAL ($abi)"

./gradlew -q :app:assembleDebug :app:assembleDebugAndroidTest -Parugula.abis="$abi"
adb install -r -t app/build/outputs/apk/debug/app-debug.apk >/dev/null
adb install -r -t app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk >/dev/null
adb shell run-as $pkg rm -rf files/handoff

case "$url" in
  http://127.0.0.1:* | http://localhost:*) p=${url##*:}; adb reverse "tcp:${p%%/*}" "tcp:${p%%/*}" >/dev/null ;;
esac
rm -rf "$handoff"; mkdir -p "$handoff"
args=(-e control "$url" -e machine "$machine" -e room "$room")
# adb shell joins its arguments into one command line: an empty one would
# vanish, and a question's words would split, so each is quoted for the
# device's shell, and left out when empty.
for kv in "board=${ARUGULA_BOARD:-}" "ask=${ARUGULA_ASK:-}" "team=${ARUGULA_TEAM:-}" "grant_board=${ARUGULA_GRANT_BOARD:-}" "run_board=${ARUGULA_RUN_BOARD:-}" \
  "todo=${ARUGULA_TODO:-}" "todo_note=${ARUGULA_TODO_NOTE:-}"; do
  [ -z "${kv#*=}" ] || args+=(-e "${kv%%=*}" "'${kv#*=}'")
done

adb shell am instrument -w -r "${args[@]}" -e class io.arugula.app.AppFlowTest $pkg.test/androidx.test.runner.AndroidJUnitRunner > "$log/run.out" 2>&1 &
run=$!
# The ticket out of the app's files, the callback in.
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
wait $run || true
for f in $(adb shell run-as $pkg ls files/handoff 2>/dev/null | tr -d '\r' | grep '\.png$' || true); do
  adb exec-out run-as $pkg cat "files/handoff/$f" > "$handoff/$f"
done
grep -E "^(INSTRUMENTATION_STATUS: (test|stack)=|OK \(|FAILURES|Tests run)|AssertionError|no .* after" "$log/run.out" | grep -v "stack=$" | tail -40
cp "$log/run.out" "$handoff/instrument.out"
grep -q '^OK (' "$log/run.out"
