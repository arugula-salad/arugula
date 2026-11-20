# The iPhone and iPad app

SwiftUI on the Rust core in `../core`, which holds signing in, the encrypted
channel to a machine and the terminal ([aru-132](../../docs/decisions/)). The core crosses
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
  Mac. Its UI test signs in, reads projects, posts in chat and runs a command in
  a relayed terminal. `ARUGULA_SIM_FAMILY=iPad` on the Mac runs it on an iPad.
- **Signing in** is the desktop app's and the CLI's way (#999): the app opens
  control's page in a web sheet (`ASWebAuthenticationSession`), someone signed
  in clicks Allow, and the page sends a one-time grant to `arugula://signin`.
  The app swaps it, with a verifier only it holds, for a session it keeps in the
  Keychain. A machine is reached with a machine token from control, to the Noise
  key control's directory lists for it. It needs `ARUGULA_NATIVE_CLIENTS` on the
  hub; without it the app says this Arugula isn't taking its sign-in yet.

## Push

Notifications come through APNs (#1260), encrypted for this device the way
a browser's Web Push is, so neither control nor Apple reads them:

- **Subscribing:** signed in, the app asks to notify, and APNs hands it a
  device token. It subscribes with control (`POST /api/push/subscribe`, which
  a native login may call for an APNs device and nothing else) at
  `https://api.push.apple.com/3/device/<token>`, or `api.sandbox.push.apple.com`
  when the build's profile says `aps-environment` development, with an ECDH
  key and a secret it makes once (RFC 8291's `p256dh` and `auth`). They're
  kept in the keychain group `HNDPMQPF22.io.arugula.ios.shared`, which the
  extension shares. Signing out unsubscribes it.
- **Sending:** daemons encrypt for that subscription and relay it as for any
  other; control sends APNs an alert that says only that something came,
  with the ciphertext as `e`.
- **Showing it:** `ArugulaNotify`, a Notification Service Extension, decrypts
  `e` with CryptoKit (`Shared/WebPush.swift`) and shows the real title and
  body; one per pane or room, the newest replacing the last.
- **Tapping it** opens what it's about: a pane on a machine (an agent's
  question), a chat room or an issue.

`PushTests/run.sh` checks the decrypting on a Mac against RFC 8291's example
and a daemon's own bytes (`just native-ios` runs it). The App IDs need Push
Notifications (`io.arugula.ios`) and to exist (`io.arugula.ios.notify`):
`build.sh` with `ASC_KEY_PATH`, `ASC_KEY_ID` and `ASC_ISSUER_ID` set signs
with the App Store Connect key in place of Xcode's account.

## TestFlight

`just native-testflight` on geek uploads `origin/main` to TestFlight (#603).
It checks the commit out in jake-air's `~/src/arugula-testflight`, archives it
there (`native/ios/build.sh archive`), and signs and uploads it with
`native/ios/testflight.sh`. It then waits until TestFlight has processed the
build. Testers see it once it's processed.

- **The key:** an App Store Connect API key with the App Manager role, in
  Infisical (project arugula, env prod, `/illogical/release`) as `APPLE_ASC_KEY_ID`,
  `APPLE_ASC_ISSUER_ID` and `APPLE_ASC_KEY_P8` (Team Key "arugula-testflight"). The script reads them while it runs and
  passes the .p8 over ssh to a temporary file on the Mac, which only the user
  can read and which is overwritten and removed afterwards. `xcodebuild -allowProvisioningUpdates
  -authenticationKey…` uses it to make the distribution signing and to
  upload, so no Apple ID is signed in. Other secret names:
  `ASC_KEY_ID_SECRET=… ASC_ISSUER_ID_SECRET=… ASC_KEY_P8_SECRET=…`.
- **Build numbers:** App Store Connect gives each upload the next number
  (`manageAppVersionAndBuildNumber`), so the project's `CURRENT_PROJECT_VERSION`
  stays at 1. The marketing version is `MARKETING_VERSION` in
  `project.pbxproj`. Raise it for a release testers should see as new.
- **Without uploading:** `just native-testflight export` signs the app for the
  App Store with Xcode's signed-in account and leaves the `.ipa` in jake-air's
  `~/src/arugula-testflight/native/ios/build/export`.
- **Another commit or Mac:** `just native-testflight upload REV HOST`.

No CI job runs this yet.

**Too old for control.** The core's `NATIVE_API` is what this build reads and
sends of control's native API. Control's `/control.json` names the oldest it
still serves (`native_api_min`). When that's newer, the app shows "Update
Arugula" in place of everything else. It checks as it opens and when it comes
back, at most every ten minutes, and before a sign-in. Raise `NATIVE_API`
with a change an older build can't follow. After that, control raises
`native_api_min` only once no supported build needs the older one: the
current build and the one before it (`contract/README.md`).

What isn't here yet: live chat (an open room looks again every few seconds), text
selection in the terminal, sessions, and approvals (from a notification too).
