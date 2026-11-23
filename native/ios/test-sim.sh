#!/bin/bash
# The app's UI test on a throwaway simulator, against the control at URL.
# Something else allows the sign-in: the UI test writes its ticket to
# HANDOFF/ticket, and waits for the address the Allow was sent to in
# HANDOFF/callback.
# The cloud repository's testnet native-sim claim drives it. ARUGULA_SIM_FAMILY=iPad
# runs it on an iPad (the side-by-side layout) instead of an iPhone.
# ARUGULA_DOC is a Doc whose card the room ARUGULA_DOC_ROOM (a name, under
# home's people) has (Arugula #1259): the test opens it, comments on its
# paragraph ARUGULA_DOC_PARA and taps the prototype's button ARUGULA_DOC_PROTO,
# whose page then says "prototype ran".
# ARUGULA_BOARD names a board home should show, and ARUGULA_ASK its row's
# question to the person signed in. ARUGULA_AGENT=1: a stand-in agent on
# MACHINE asks for a permission and a question, for home to answer.
# ARUGULA_CARD_AGENT=1: that agent works on ARUGULA_BOARD's card, so the
# Board level shows it on the ticket (Arugula #1306).
# ARUGULA_STAFF_PROJECT names a team's project where the session
# ARUGULA_STAFF_LEAD (cos) leads the person's staff room and the board
# ARUGULA_STAFF_BOARD (Arugula #1279): the test asks the chief of staff, opens
# the board's thread from its lead on Boards, and peeks at the lead from both.
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
# XCTest's own queries and Accessibility's answers to them log in the app's
# process, over a hundred thousand lines in a run on an iPad: enough that iOS
# quarantines the app for logging volume, and XCTest's runtime-issue callback
# then crashes it (Arugula #1282). Off for this device; the test's own record
# is xcodebuild's log.
xcrun simctl boot "$device"
xcrun simctl bootstatus "$device" >/dev/null
for subsystem in com.apple.dt.xctest com.apple.Accessibility; do
  xcrun simctl spawn "$device" log config --subsystem "$subsystem" --mode level:off
done
TEST_RUNNER_ARUGULA_CONTROL_URL="$url" TEST_RUNNER_ARUGULA_MACHINE="$machine" TEST_RUNNER_ARUGULA_HANDOFF="$handoff" \
  TEST_RUNNER_ARUGULA_BOARD="${ARUGULA_BOARD:-}" TEST_RUNNER_ARUGULA_ASK="${ARUGULA_ASK:-}" TEST_RUNNER_ARUGULA_AGENT="${ARUGULA_AGENT:-}" \
  TEST_RUNNER_ARUGULA_CARD_AGENT="${ARUGULA_CARD_AGENT:-}" \
  TEST_RUNNER_ARUGULA_DOC="${ARUGULA_DOC:-}" TEST_RUNNER_ARUGULA_DOC_ROOM="${ARUGULA_DOC_ROOM:-}" \
  TEST_RUNNER_ARUGULA_DOC_PARA="${ARUGULA_DOC_PARA:-}" TEST_RUNNER_ARUGULA_DOC_PROTO="${ARUGULA_DOC_PROTO:-}" \
  TEST_RUNNER_ARUGULA_STAFF_PROJECT="${ARUGULA_STAFF_PROJECT:-}" TEST_RUNNER_ARUGULA_STAFF_BOARD="${ARUGULA_STAFF_BOARD:-}" \
  TEST_RUNNER_ARUGULA_STAFF_LEAD="${ARUGULA_STAFF_LEAD:-}" \
  xcodebuild test -project Arugula.xcodeproj -scheme Arugula -sdk iphonesimulator \
  -destination "platform=iOS Simulator,id=$device" -derivedDataPath build/dd \
  CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM= \
  > "$handoff/xcodebuild.log" 2>&1 && rc=0 || rc=$?
grep -E 'error:|XCTAssert|Test Case .* (passed|failed)|\*\* TEST' "$handoff/xcodebuild.log" | tail -20
exit "$rc"
