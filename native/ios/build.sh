#!/bin/bash
# Builds the Rust core and the app. On a Mac with Xcode, rustup and mise.
#   native/ios/build.sh sim              for the iOS simulator
#   native/ios/build.sh device [DEVICE]  for an iPhone, signed with the team's
#                                        development profile; with a devicectl
#                                        id, installs and launches it
# Signing needs the logged-in GUI session's keychain, so from an ssh shell run
# it through native/ios/gui.sh.
set -euo pipefail
cd "$(dirname "$0")"
kind=${1:?usage: build.sh sim|device [DEVICE]}
device=${2:-}
export PATH="$HOME/.cargo/bin:$HOME/.local/bin:/opt/homebrew/bin:$PATH"
export IPHONEOS_DEPLOYMENT_TARGET=17.0
case "$kind" in
  sim) target=aarch64-apple-ios-sim sdk=iphonesimulator ;;
  device) target=aarch64-apple-ios sdk=iphoneos ;;
  *) echo "build.sh: sim or device" >&2; exit 2 ;;
esac
out=$PWD/build
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$out/cargo}"
rustup target add "$target" >/dev/null
(cd ../core && mise exec -- cargo build --release --lib --target "$target")
mkdir -p "$out/lib/$sdk"
cp "$CARGO_TARGET_DIR/$target/release/libarugula_native_core.a" "$out/lib/$sdk/"
if [ "$kind" = sim ]; then
  echo "built the core for the simulator: $out/lib/$sdk"
  exit 0
fi
# By target, with no destination: naming one needs Xcode's copy of the phone's
# iOS platform, which signing and installing don't.
xcodebuild -project Arugula.xcodeproj -target Arugula -configuration Debug -sdk iphoneos -allowProvisioningUpdates \
  SYMROOT="$out/xb" OBJROOT="$out/xb/obj" DEVELOPMENT_TEAM="${DEVELOPMENT_TEAM:-HNDPMQPF22}" build \
  | grep -E '^\*\* |error:|warning: unre|Signing|requires a provisioning' || true
app="$out/xb/Debug-iphoneos/Arugula.app"
[ -d "$app" ] || { echo "build.sh: no app was built" >&2; exit 1; }
codesign -dv "$app" 2>&1 | grep -E 'Identifier|TeamIdentifier' || true
if [ -n "$device" ]; then
  xcrun devicectl device install app --device "$device" "$app"
  xcrun devicectl device process launch --device "$device" io.arugula.ios || echo "installed, but it wouldn't launch: is the phone unlocked?"
fi
