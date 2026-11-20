#!/bin/bash
# Push's decrypting (Shared/WebPush.swift) against RFC 8291's example and a
# daemon's notification: main.swift's checks, on a Mac (CryptoKit is Apple's).
# `just native-ios` runs it.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p ../build
xcrun swiftc -O ../Shared/WebPush.swift main.swift -o ../build/push-tests
../build/push-tests
