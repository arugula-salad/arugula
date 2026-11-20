#!/bin/bash
# The app's UI test on a throwaway simulator, against the control at URL.
# Something else allows the sign-in: the UI test writes its ticket to
# HANDOFF/ticket, and waits for the address the Allow was sent to in
# HANDOFF/callback.
# The cloud repository's testnet native-sim claim drives it. ARUGULA_SIM_FAMILY=iPad
# runs it on an iPad (the side-by-side layout) instead of an iPhone.
# ARUGULA_BOARD names a board home should show, and ARUGULA_ASK its row's
# question to the person signed in. ARUGULA_AGENT=1: a stand-in agent on
# MACHINE asks for a permission and a question, for home to answer.
#   native/ios/test-sim.sh URL MACHINE HANDOFF
set -euo pipefail
cd "$(dirname "$0")"
url=${1:?usage: test-sim.sh URL MACHINE HANDOFF} machine=${2:?} handoff=${3:?}
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:/opt/homebrew/bin:$PATH"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/arugula-native-core-target}"
rm -rf "$handoff"; mkdir -p "$handoff"
./build.sh sim >/dev/null
pick=$(xcrun simctl list -j runtimes available | ARUGULA_SIM_FAMILY="${ARUGULA_SIM_FAMILY:-iPhone}" python3 -c '
import json, os, sys
family = os.environ["ARUGULA_SIM_FAMILY"]
ios = [r for r in json.load(sys.stdin)["runtimes"] if r["platform"] == "iOS"]
kinds = [t for r in ios[-1:] for t in r["supportedDeviceTypes"] if t["productFamily"] == family]
if not kinds:
    sys.exit(f"no {family} simulator in the newest iOS runtime")
print(ios[-1]["identifier"], kinds[-1]["identifier"])
')
read -r runtime type <<<"$pick"
device=$(xcrun simctl create arugula-ui-test "$type" "$runtime")
trap 'xcrun simctl shutdown "$device" 2>/dev/null || true; xcrun simctl delete "$device"' EXIT
TEST_RUNNER_ARUGULA_CONTROL_URL="$url" TEST_RUNNER_ARUGULA_MACHINE="$machine" TEST_RUNNER_ARUGULA_HANDOFF="$handoff" \
  TEST_RUNNER_ARUGULA_BOARD="${ARUGULA_BOARD:-}" TEST_RUNNER_ARUGULA_ASK="${ARUGULA_ASK:-}" TEST_RUNNER_ARUGULA_AGENT="${ARUGULA_AGENT:-}" \
  xcodebuild test -project Arugula.xcodeproj -scheme Arugula -sdk iphonesimulator \
  -destination "platform=iOS Simulator,id=$device" -derivedDataPath build/dd \
  CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM= \
  > "$handoff/xcodebuild.log" 2>&1 && rc=0 || rc=$?
grep -E 'error:|XCTAssert|Test Case .* (passed|failed)|\*\* TEST' "$handoff/xcodebuild.log" | tail -20
exit "$rc"
