---
section: Changed
title: Updates check a signed checksum list
line: Update now, arugulad update and install.sh check a release against its signed SHA256SUMS, and install.sh checks the Mac app's signature.
headline: no
---
A daemon release's `SHA256SUMS` now comes with `SHA256SUMS.sig`, signed by the
release key. `arugulad update` and the web notice's *Update now* refuse a release
whose checksum list that key didn't sign. `install.sh` checks the signature too
where openssl has Ed25519 (most Linux), and stops when a release has none unless
`ARUGULA_ALLOW_UNSIGNED=1` (for releases from before signing, or a local build).
On a Mac it checks that the app is signed by Arugula's Developer ID before moving
it into Applications.
