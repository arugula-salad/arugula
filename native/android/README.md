# Android

The Android app, and the Rust core in `../core` (signing in, the channel to a
machine, the terminal) for Kotlin, which it runs on.

- `app/`: the app (#1249), Jetpack Compose on Material 3 in Arugula's look. It
  does what the iPhone app does: signs in through a Custom Tab and the
  `arugula://signin` callback, keeping the session encrypted under a Keystore
  key in `noBackupFilesDir`; the web's five places on a bar that's always
  there, Overview · Boards · Docs · Machines · Chat (a navigation rail on a
  tablet, #1362), each headed by the project's menu, New, the bell (the
  Inbox) and your picture (your name, Settings in a web view, Send feedback,
  the version and Sign out: `Account.kt`); the Overview's Needs you (answered
  in the row), New ideas, boards and people; Docs, a word that #1364 lists
  them; a board's thread and lanes; chat rooms on the
  live stream; Machines as one page of machine › session › pane rows (beside
  the session's live tab on a tablet), a session a tab at a time, and a
  pane's terminal. Its fonts are fetched
  into the build at a pinned commit and checked, as the iPhone's are. A debug
  build reads `-Parugula.control=URL` for another control.
- `core/`: an Android library. Its Kotlin (`io.arugula.core`) calls the core
  over JNI (`../core/src/kotlin.rs`), which goes through the same `aru_*`
  functions the iPhone app's Swift calls, so both apps behave the same. Its
  Gradle build runs `cargo ndk` for each ABI in `arugula.abis`
  (`gradle.properties`): `arm64-v8a` for phones, `x86_64` for the emulator.
- `setup.sh`: the toolchain, into your home: the SDK's command-line tools,
  platform 36, build tools, NDK r29, the emulator and an image, the AVD
  `arugula-core`, Gradle 9.9.2 (which `gradlew` runs: the repository keeps no
  binaries, so no wrapper jar), Rust's two Android targets and cargo-ndk 4.1.2. Run it again
  to check. `setup.sh emulator` installs only what an emulator needs.
- `test-emulator.sh`: the instrumented tests (`core/src/androidTest`) on a
  device: the one adb already has (`ANDROID_SERIAL`), else the AVD, booted
  headless and shut after.

## What Kotlin can do

| | |
|---|---|
| Sign in | `SignIn.start(url, name)`; open `link` in a Custom Tab; when the page sends the browser to `arugula://signin?ticket=…&grant=…`, `finish(thatAddress)` gives the session's JSON, to keep (encrypted with a Keystore key). |
| Session calls | `Control(session).get("/api/projects")`, `post(...)`, `call(method, path, body)`: control's JSON. A refusal throws `CoreError` (`status`, `reason`; `signedOut` means sign in again). `ChatLive(control) { … }` is chat's live stream. |
| A machine | `Link(control, machineId) { … }`: `feed(panes, terms)` takes what's queued into each pane's `Term`, `panes` is the machine's panes, tabs and sessions as JSON, `attach`/`watch`/`view` choose what streams, `request` calls the machine's own API. |
| Input | `link.input(pane, bytes)`; `term.key(Key.ENTER, mods, text)` gives a key's bytes under the terminal's modes. |
| The terminal | `term.render()`: the rows that changed, as runs of cells with colors and flags, and the cursor. |

The `Notify` callbacks come on the core's own threads: hop to the main thread
before touching the UI. Calls that reach the network block. Call
`Core.init(context)` once first: TLS checks control's certificate against
Android's trust store through it (rustls-platform-verifier). Device keys play
no part: a machine is reached with a machine token from control.

## Running it

```sh
native/android/setup.sh             # once per machine, and after a pin changes
export ANDROID_HOME=~/Android/Sdk
just native-android                 # both ABIs and the app, then the emulator tests
```

The app's debug APK, for a phone (arm64) and the emulator:

```sh
native/android/gradlew :app:assembleDebug   # app/build/outputs/apk/debug/app-debug.apk
adb install -r native/android/app/build/outputs/apk/debug/app-debug.apk
```

On Linux the emulator needs `/dev/kvm`, so the user has to be in the `kvm`
group (`sudo usermod -aG kvm $USER`, then log in again).

The emulator tests with no Arugula are the terminal's and TLS's (against
control.arugula.io; `ARUGULA_NO_TLS_CHECK=1` skips it). `native-android.yml` runs
them nightly on a GitHub runner. The whole flow (sign in a fresh account
through the native callback, list projects, post in chat, and type into a
relayed pane) runs against the cloud repository's test stack:

```sh
native/android/test-emulator.sh http://127.0.0.1:PORT box-systemd ROOM /tmp/handoff
```

It writes the sign-in's ticket to `HANDOFF/ticket` and waits for the address
the Allow was sent to in `HANDOFF/callback`, as the iPhone's `test-sim.sh` does.

The app's own UI test (`app/src/androidTest`) taps the same flow through the
app, as the iPhone's `FlowTests.swift` does: sign in, the five places and
the heading's bell and picture, the board's question answered on the
Overview, a message in chat, a command in a new terminal, Docs, and sign out
from your picture. Testnet's `native-android` claim runs it, with the same arguments:

```sh
ARUGULA_BOARD=TITLE ARUGULA_ASK=QUESTION \
  native/android/test-app.sh http://127.0.0.1:PORT box-systemd ROOM /tmp/handoff
```

Its screenshots land in the handoff directory.
