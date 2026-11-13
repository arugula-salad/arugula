# The iPhone app

SwiftUI on the Rust core in `../core`, which holds signing, the encrypted channel
to a machine and the terminal ([aru-132](../../docs/decisions/)). The core crosses
to Swift through a small C interface, `Arugula/arugula_core.h` and
`../core/src/ffi.rs`, mostly as JSON.

- **On a phone:** `just native-phone DEVICE` on a Mac signed in to the Apple team
  (`xcrun devicectl list devices` has the id). It builds, signs with the team's
  development profile, installs and launches.
- **On a simulator, end to end:** `ARUGULA_NATIVE_SIM_HOST=MAC just testnet test
  control native-sim`, with this branch at `~/src/native-app` on the Mac. Its UI
  test pairs, reads projects, posts in chat and runs a command in a relayed
  terminal, and control's stack is tunnelled to the Mac. Screenshots land in
  `ARUGULA_NATIVE_SIM_OUT`.
- **Pairing needs** `ARUGULA_NATIVE_CLIENTS` on the hub. Without it the app says
  this Arugula isn't taking it yet.

What isn't here yet: live chat (an open room looks again every few seconds), text
selection in the terminal, machines of other accounts or hosted ones (the app only
trusts its own account's), sessions, approvals and push.
