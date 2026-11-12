# The iPhone and iPad app

SwiftUI on the Rust core in `../core`, which holds signing, the encrypted channel
to a machine and the terminal ([aru-132](../../docs/decisions/)). The core crosses
to Swift through a small C interface, `Arugula/arugula_core.h` and
`../core/src/ffi.rs`, mostly as JSON.

One app for both: on an iPhone each tab is one screen at a time, and on an iPad
a list sits beside what's open from it (a project, a chat room, a machine).

- **On a phone or an iPad:** `just native-phone DEVICE` on a Mac signed in to the
  Apple team (`xcrun devicectl list devices` has the id). It builds, signs with
  the team's development profile, installs and launches. A new device has to be
  connected to that Mac once, trusted, and put in Developer Mode.
- **On a simulator, end to end:** control's hub lives in the cloud repository, so
  its testnet drives this: `ARUGULA_NATIVE_SIM_HOST=MAC just testnet test control
  native-sim` there, with this repository's branch at `~/src/native-app` on the
  Mac. Its UI test pairs, reads projects, posts in chat and runs a command in a
  relayed terminal. `ARUGULA_SIM_FAMILY=iPad` on the Mac runs it on an iPad.
- **Pairing needs** `ARUGULA_NATIVE_CLIENTS` on the hub. Without it the app says
  this Arugula isn't taking it yet.

What isn't here yet: live chat (an open room looks again every few seconds), text
selection in the terminal, machines of other accounts or hosted ones (the app only
trusts its own account's), sessions, approvals and push.
