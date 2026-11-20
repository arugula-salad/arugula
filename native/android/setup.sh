#!/bin/bash
# Android's toolchain for the core and its emulator test, in this user's home
# (no system packages): the SDK's command-line tools, platform, build tools,
# NDK, emulator and a system image, an AVD called arugula-core, Rust's two
# Android targets and cargo-ndk. Run it again to check; it only adds what's
# missing.
#
#   native/android/setup.sh             everything (Linux or macOS)
#   native/android/setup.sh emulator    only what an emulator needs: a Mac
#                                       that runs it for another machine's adb
#
# ANDROID_HOME defaults to ~/Android/Sdk. The emulator's image is x86_64 on an
# x86_64 host (which needs /dev/kvm) and arm64-v8a on Apple silicon.
set -euo pipefail
only=${1:-all}
sdk=${ANDROID_HOME:-$HOME/Android/Sdk}
# Where the AVD goes, said outright: avdmanager otherwise picks
# ~/.config/.android when ~/.android doesn't exist, where the emulator
# doesn't look. test-emulator.sh says the same.
export ANDROID_AVD_HOME=${ANDROID_AVD_HOME:-$HOME/.android/avd}
mkdir -p "$ANDROID_AVD_HOME"

# What's pinned. sdkmanager installs the newest emulator and platform-tools
# there are; these paths are the versions that matter to the build.
TOOLS_BUILD=15641748 # cmdline-tools 19.0
NDK=29.0.14206865    # r29, as core/build.gradle.kts's ndkVersion
PLATFORM=android-36
BUILD_TOOLS=36.0.0
CARGO_NDK=4.1.2

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64) os=linux abi=x86_64 ;;
  Darwin-arm64) os=mac abi=arm64-v8a ;;
  *) echo "setup.sh: no Android emulator image for $(uname -sm)" >&2; exit 1 ;;
esac
image="system-images;$PLATFORM;google_apis;$abi"

if [ ! -x "$sdk/cmdline-tools/latest/bin/sdkmanager" ]; then
  echo "setup.sh: command-line tools into $sdk"
  tmp=$(mktemp -d)
  curl -fsSLo "$tmp/tools.zip" "https://dl.google.com/android/repository/commandlinetools-$os-${TOOLS_BUILD}_latest.zip"
  unzip -q "$tmp/tools.zip" -d "$tmp"
  mkdir -p "$sdk/cmdline-tools"
  rm -rf "$sdk/cmdline-tools/latest"
  mv "$tmp/cmdline-tools" "$sdk/cmdline-tools/latest"
  rm -rf "$tmp"
fi
sdkmanager() { "$sdk/cmdline-tools/latest/bin/sdkmanager" --sdk_root="$sdk" "$@"; }

yes | sdkmanager --licenses >/dev/null 2>&1 || true
packages=(platform-tools emulator "$image")
[ "$only" = emulator ] || packages+=("platforms;$PLATFORM" "build-tools;$BUILD_TOOLS" "ndk;$NDK")
sdkmanager --install "${packages[@]}" | grep -v '^\[' || true

if ! "$sdk/cmdline-tools/latest/bin/avdmanager" list avd -c | grep -qx arugula-core; then
  echo no | "$sdk/cmdline-tools/latest/bin/avdmanager" create avd -n arugula-core -k "$image" -d pixel_7 >/dev/null
  echo "setup.sh: made the AVD arugula-core ($image)"
fi

if [ "$os" = linux ] && [ ! -w /dev/kvm ]; then
  echo "setup.sh: /dev/kvm isn't writable by $USER, so the emulator can't start here." >&2
  echo "  Someone with sudo: sudo usermod -aG kvm $USER (then log in again)." >&2
fi

[ "$only" = emulator ] && exit 0

rustup target add aarch64-linux-android x86_64-linux-android >/dev/null
if [ "$(cargo ndk --version 2>/dev/null | awk '{print $2}')" != "$CARGO_NDK" ]; then
  cargo install --locked "cargo-ndk@$CARGO_NDK"
fi
echo "setup.sh: ready. export ANDROID_HOME=$sdk"
