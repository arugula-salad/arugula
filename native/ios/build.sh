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
# The swarm's fonts, as the web sets them (shared/fonts.css): Big Shoulders
# Display, Instrument Sans and JetBrains Mono, SIL Open Font License 1.1, from
# Google Fonts at a pinned commit and checked, into Arugula/Fonts (git ignores
# it; the app registers what it finds there, and falls back to the system's).
fonts_rev=c30b221beb0c1da59d563feb91ec8656a1fa972a
mkdir -p Arugula/Fonts
while read -r sum path name; do
  file="Arugula/Fonts/$name"
  echo "$sum  $file" | shasum -a 256 -c --status 2>/dev/null && continue
  curl -sSfL "https://raw.githubusercontent.com/google/fonts/$fonts_rev/ofl/$path" -o "$file.part"
  echo "$sum  $file.part" | shasum -a 256 -c --status || { echo "build.sh: $name isn't the font it should be" >&2; rm -f "$file.part"; exit 1; }
  mv "$file.part" "$file"
done <<'FONTS'
60e208dc276a1c35fc5b62e94f9fb959c40c11783a9eb7548175c14b1fbeb720 bigshouldersdisplay/BigShouldersDisplay%5Bwght%5D.ttf BigShouldersDisplay.ttf
b24f1812584816958afcf22e22d08e44318c5e51651e25d2438efdde389b33b1 instrumentsans/InstrumentSans%5Bwdth,wght%5D.ttf InstrumentSans.ttf
48715a42ec242c21e9f02692891e147d022299a52e48d5e413e1a942193ffeda jetbrainsmono/JetBrainsMono%5Bwght%5D.ttf JetBrainsMono.ttf
FONTS
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
