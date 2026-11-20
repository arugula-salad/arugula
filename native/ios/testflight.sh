#!/bin/bash
# The app for the App Store: archives it, signs it for distribution and
# uploads it to App Store Connect, where TestFlight processes it. On a Mac
# signed in to the Apple team; from an ssh shell, through native/ios/gui.sh.
#   native/ios/testflight.sh upload KEY_ID ISSUER_ID KEY_PATH
#   native/ios/testflight.sh export [KEY_ID ISSUER_ID KEY_PATH]
# KEY_PATH is an App Store Connect API key's .p8 (App Manager role): with it
# xcodebuild signs and uploads with no Apple ID. export leaves the signed .ipa
# in build/export instead of uploading; without a key it uses the account
# signed in to Xcode. App Store Connect numbers the builds it's sent
# (manageAppVersionAndBuildNumber), so each upload is one higher.
# `just native-testflight` on geek runs this with the key from Infisical.
set -euo pipefail
cd "$(dirname "$0")"
mode=${1:?usage: testflight.sh upload|export [KEY_ID ISSUER_ID KEY_PATH]}
case "$mode" in
  upload) : "${4:?upload needs KEY_ID ISSUER_ID KEY_PATH}" ;;
  export) ;;
  *) echo "testflight.sh: upload or export" >&2; exit 2 ;;
esac
auth=()
if [ -n "${4:-}" ]; then
  export ASC_KEY_ID=$2 ASC_ISSUER_ID=$3 ASC_KEY_PATH=$4
  auth=(-authenticationKeyPath "$ASC_KEY_PATH" -authenticationKeyID "$ASC_KEY_ID" -authenticationKeyIssuerID "$ASC_ISSUER_ID")
fi
./build.sh archive
out=$PWD/build
options=$out/ExportOptions.plist
destination=$([ "$mode" = upload ] && echo upload || echo export)
cat > "$options" <<PL
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>method</key><string>app-store-connect</string>
<key>destination</key><string>$destination</string>
<key>teamID</key><string>${DEVELOPMENT_TEAM:-HNDPMQPF22}</string>
<key>signingStyle</key><string>automatic</string>
<key>manageAppVersionAndBuildNumber</key><true/>
<key>uploadSymbols</key><true/>
</dict></plist>
PL
rm -rf "$out/export"
xcodebuild -exportArchive -archivePath "$out/Arugula.xcarchive" -exportPath "$out/export" -exportOptionsPlist "$options" \
  -allowProvisioningUpdates ${auth[@]+"${auth[@]}"} > "$out/export.log" 2>&1 && rc=0 || rc=$?
grep -E '^\*\* |error:|Upload|Uploaded|uploaded|Progress|Exported' "$out/export.log" || true
[ "$rc" = 0 ] || { echo "testflight.sh: xcodebuild -exportArchive failed ($rc): $out/export.log" >&2; exit "$rc"; }
info=$out/Arugula.xcarchive/Products/Applications/Arugula.app/Info.plist
echo "version $(plutil -extract CFBundleShortVersionString raw "$info"), archived as build $(plutil -extract CFBundleVersion raw "$info")"
if [ "$mode" = export ]; then
  ls "$out"/export/*.ipa
else
  echo "uploaded: App Store Connect gives it the next build number and processes it for TestFlight"
fi
