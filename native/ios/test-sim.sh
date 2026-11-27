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
# ARUGULA_GRANT_BOARD, the same board, has a grant the person gave: the test
# revokes it from the lead card. ARUGULA_RUN_BOARD, a board in that project
# with no lead and no grant: the test presses Run this board on it and writes
# HANDOFF/run-board (Arugula #1456).
# ARUGULA_DEPLOY is a deploy waiting on the person (Arugula #604): the test
# opens it from Needs you and writes the address Approve opens to
# HANDOFF/approve; something else approves it with the account's passkey and
# writes HANDOFF/approved.
# ARUGULA_TODO is a stand-in agent's to-do in ARUGULA_STAFF_PROJECT (its prp_
# id, Arugula #1470): the test opens it under Needs you there and marks it
# Done with ARUGULA_TODO_NOTE.
#   native/ios/test-sim.sh URL MACHINE HANDOFF
set -euo pipefail
cd "$(dirname "$0")"
url=${1:?usage: test-sim.sh URL MACHINE HANDOFF} machine=${2:?} handoff=${3:?}
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:/opt/homebrew/bin:$PATH"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.cache/arugula-native-core-target}"
rm -rf "$handoff"; mkdir -p "$handoff"
./build.sh sim >/dev/null
pick=$(xcrun simctl list -j runtimes available | ARUGULA_SIM_FAMILY="${ARUGULA_SIM_FAMILY:-iPhone}" python3 -c '
import json, os, re, sys
family = os.environ["ARUGULA_SIM_FAMILY"]
# A general runtime, not one made for a single new device (iOS-27-2-iPhone19-4),
# whose screen the test is not made for.
ios = [r for r in json.load(sys.stdin)["runtimes"] if r["platform"] == "iOS" and re.fullmatch(r".*\.iOS-\d+-\d+", r["identifier"])]
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
# then crashes it (Arugula #1282). Off for this device, as `log config` would
# set it, before xcodebuild boots it; the test's own record is xcodebuild's log.
logging="$HOME/Library/Developer/CoreSimulator/Devices/$device/data/Library/Preferences/Logging/Subsystems"
mkdir -p "$logging"
for subsystem in com.apple.dt.xctest com.apple.Accessibility; do
  cat > "$logging/$subsystem.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict><key>DEFAULT-OPTIONS</key><dict><key>Level</key><dict><key>Enable</key><string>off</string><key>Persist</key><string>off</string></dict></dict></dict>
</plist>
PLIST
done
# A notification the UI test asks for (HANDOFF/push.json: what it says, as
# the Notification Service Extension leaves it for the app), delivered as
# APNs would.
(
  while kill -0 $$ 2>/dev/null; do
    if [ -f "$handoff/push.json" ]; then
      python3 -c 'import json, sys
said = open(sys.argv[1]).read()
s = json.loads(said)
print(json.dumps({"aps": {"alert": {"title": s["title"], "body": s["body"]}, "sound": "default"}, "said": said}))' \
        "$handoff/push.json" > "$handoff/push.apns" && rm -f "$handoff/push.json"
      xcrun simctl push "$device" io.arugula.ios "$handoff/push.apns" >/dev/null
    fi
    sleep 1
  done
) &
TEST_RUNNER_ARUGULA_CONTROL_URL="$url" TEST_RUNNER_ARUGULA_MACHINE="$machine" TEST_RUNNER_ARUGULA_HANDOFF="$handoff" \
  TEST_RUNNER_ARUGULA_BOARD="${ARUGULA_BOARD:-}" TEST_RUNNER_ARUGULA_ASK="${ARUGULA_ASK:-}" TEST_RUNNER_ARUGULA_AGENT="${ARUGULA_AGENT:-}" \
  TEST_RUNNER_ARUGULA_CARD_AGENT="${ARUGULA_CARD_AGENT:-}" \
  TEST_RUNNER_ARUGULA_DOC="${ARUGULA_DOC:-}" TEST_RUNNER_ARUGULA_DOC_ROOM="${ARUGULA_DOC_ROOM:-}" \
  TEST_RUNNER_ARUGULA_DOC_PARA="${ARUGULA_DOC_PARA:-}" TEST_RUNNER_ARUGULA_DOC_PROTO="${ARUGULA_DOC_PROTO:-}" \
  TEST_RUNNER_ARUGULA_STAFF_PROJECT="${ARUGULA_STAFF_PROJECT:-}" TEST_RUNNER_ARUGULA_STAFF_BOARD="${ARUGULA_STAFF_BOARD:-}" \
  TEST_RUNNER_ARUGULA_STAFF_LEAD="${ARUGULA_STAFF_LEAD:-}" TEST_RUNNER_ARUGULA_DEPLOY="${ARUGULA_DEPLOY:-}" \
  TEST_RUNNER_ARUGULA_GRANT_BOARD="${ARUGULA_GRANT_BOARD:-}" TEST_RUNNER_ARUGULA_RUN_BOARD="${ARUGULA_RUN_BOARD:-}" \
  TEST_RUNNER_ARUGULA_TODO="${ARUGULA_TODO:-}" TEST_RUNNER_ARUGULA_TODO_NOTE="${ARUGULA_TODO_NOTE:-}" \
  xcodebuild test -project Arugula.xcodeproj -scheme Arugula -sdk iphonesimulator \
  -destination "platform=iOS Simulator,id=$device" -derivedDataPath build/dd \
  CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM= \
  > "$handoff/xcodebuild.log" 2>&1 && rc=0 || rc=$?
grep -E 'error:|XCTAssert|Test Case .* (passed|failed)|\*\* TEST' "$handoff/xcodebuild.log" | tail -20
exit "$rc"
