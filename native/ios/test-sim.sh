#!/bin/bash
# The app's UI test on a throwaway simulator, against the control at URL.
# Something else approves the pairing: this writes its code to HANDOFF/code.
# testnet's native-sim claim drives it.
#   native/ios/test-sim.sh URL MACHINE HANDOFF
set -euo pipefail
cd "$(dirname "$0")"
url=${1:?usage: test-sim.sh URL MACHINE HANDOFF} machine=${2:?} handoff=${3:?}
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:/opt/homebrew/bin:$PATH"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/arugula-native-core-target}"
rm -rf "$handoff"; mkdir -p "$handoff"
./build.sh sim >/dev/null
pick=$(xcrun simctl list -j runtimes available | python3 -c '
import json, sys
ios = [r for r in json.load(sys.stdin)["runtimes"] if r["platform"] == "iOS"]
phones = [t for r in ios[-1:] for t in r["supportedDeviceTypes"] if t["productFamily"] == "iPhone"]
print(ios[-1]["identifier"], phones[-1]["identifier"])
')
read -r runtime type <<<"$pick"
device=$(xcrun simctl create arugula-ui-test "$type" "$runtime")
trap 'xcrun simctl shutdown "$device" 2>/dev/null || true; xcrun simctl delete "$device"' EXIT
TEST_RUNNER_ARUGULA_CONTROL_URL="$url" TEST_RUNNER_ARUGULA_MACHINE="$machine" TEST_RUNNER_ARUGULA_HANDOFF="$handoff" \
  xcodebuild test -project Arugula.xcodeproj -scheme Arugula -sdk iphonesimulator \
  -destination "platform=iOS Simulator,id=$device" -derivedDataPath build/dd \
  CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM= \
  > "$handoff/xcodebuild.log" 2>&1 && rc=0 || rc=$?
grep -E 'error:|XCTAssert|Test Case .* (passed|failed)|\*\* TEST' "$handoff/xcodebuild.log" | tail -20
exit "$rc"
