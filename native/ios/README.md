# The iPhone and iPad app

SwiftUI on the Rust core in `../core`, which holds signing in, the encrypted
channel to a machine and the terminal ([aru-132](../../docs/decisions/)). The core crosses
to Swift through a small C interface, `Arugula/arugula_core.h` and
`../core/src/ffi.rs`, mostly as JSON.

One app for both: on an iPhone each screen is one at a time, and on an iPad
a list sits beside what's open from it (a project, a chat room, a machine).

The tab bar is always there (#1262): **Home · Boards · Chat · Machines · You**.
Issues aren't a tab: a board's rows open their issues, and the project's menu
(the project name heading Home, Boards and Chat) opens its list.

**Home** (`Arugula/Home.swift`, #1255) is the first tab: the current project's
conversations, sorted by what needs you, as the desktop's Overview reads them.
Needs you merges every online machine's panes that ask or failed (a link to
each, as the web's fleet opens them), the boards' rows that ask you, fail
their checks or wait for review, and the hosted sessions that wait on you,
worst first. Then one row per open board (its lane bar, `done/total`, what's
going on and who leads it), which opens the board's thread, whose Board
button opens its lanes; then the channels and DMs. On an iPad home is the
left column, and what it opens sits beside it. A notification opens its tab:
a pane's on Machines, a room's on Chat, an issue's beside home.

A row of Needs you is **answered where it is** (`Arugula/Answer.swift`, #1256),
and folds away with a word of what was done: a pane's permission (Allow,
Deny) or question (its choices) through its daemon's `POST
/api/attention/act` over the link, a hosted session's through control's
`POST /api/sessions/{ses}/actions`, a board's ask through its answer route.
Tapping a session, in Needs you or on a board's thread, opens its **peek**:
its last lines, a reply to its agent, Silence (the owner's: teammates'
agents can't mention it), Stop (asked first) and its terminal or thread. A
viewer of a machine's session reads it, with no buttons.

**New** on home (`Arugula/New.swift`, #1257) tells a board (its thread opens,
and a post there reaches the session that leads it) or starts a **New idea**.
What you say (typed, or dictated with the keyboard's mic) becomes a native
issue, "Idea: …", whose body holds the agent's instructions (ask at most two
questions in the thread, write a project Doc, with a prototype when one fits,
post its link, and make a board when asked) and your words. `POST
/api/issues/{issue}/agent` then starts a hosted session on it, as Agent on
this does, and its thread is the conversation, listed under New ideas. When
Arugula cloud can't run it for the account (no hosted workspace, no standing
grant), the app says why and offers your online machines. There it starts a
Claude Code agent block on the same issue (`POST /api/blocks` over the link),
told to talk in the issue's thread.

A Doc linked in a message is a card that opens the Doc's page from control
(`/embed/p/:project/docs/:doc`) in a web view (`DocViews.swift`). Control
sets no cookie there: the app puts its own session's cookie in the view's
store, so the page reads and comments as the app, a native session. The
page's links to a board, an issue, a room or another Doc come back as intents
on `window.webkit.messageHandlers.arugula` and open in the app. On an iPhone
the Doc is over everything; on an iPad it's a third column beside the
conversation, home's list folded away until Close. Debug builds allow plain
http to `*.localhost` (`Arugula-Debug-Info.plist`) so a testnet's prototype
runner loads.

- **On a phone or an iPad:** `just native-phone DEVICE` on a Mac signed in to the
  Apple team (`xcrun devicectl list devices` has the id). It builds, signs with
  the team's development profile, installs and launches. A new device has to be
  connected to that Mac once, trusted, and put in Developer Mode.
- **On a simulator, end to end:** control's hub lives in the cloud repository, so
  its testnet drives this: `ARUGULA_NATIVE_SIM_HOST=MAC just testnet test control
  native-sim` there, with this repository's branch at `~/src/native-app` on the
  Mac (`ARUGULA_NATIVE_SIM_DIR` names another). Its UI test signs in, finds the
  board the claim made on home with its row's question under Needs you, opens
  the board's thread, posts there and opens its lanes; peeks at a stand-in
  agent, allows its permission prompt and answers its question and the
  board's from home (the claim checks the agent went on); tells the board and
  starts a New idea (testnet has no hosted sessions, so it runs on
  box-systemd, which the claim checks), then reads projects,
  posts in chat and runs a command in a relayed terminal.
  `ARUGULA_SIM_FAMILY=iPad` on the Mac runs it on an iPad.
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
It checks the commit out in jake-mini's `~/src/arugula-testflight`. That's a
clone of its own, so the testnet simulator claim's `~/src/native-app` is left
alone. It archives there, unsigned (`native/ios/build.sh archive`), and
`native/ios/testflight.sh` signs and uploads it. It then waits until
TestFlight has processed the build. Testers see it once it's processed.

- **The key:** an App Store Connect API key with the App Manager role, in
  Infisical (project arugula, env prod, `/illogical/release`) as
  `APPLE_ASC_KEY_ID`, `APPLE_ASC_ISSUER_ID` and `APPLE_ASC_KEY_P8` (Team Key
  "arugula-testflight").
  - The script reads them while it runs, and passes the .p8 over ssh to a
    temporary file on the Mac that only the user can read. It's overwritten
    and removed afterwards.
  - With it, `xcodebuild -exportArchive -allowProvisioningUpdates
    -authenticationKey…` signs with the team's cloud-managed distribution
    certificate and uploads. No Apple ID is signed in, and the Mac's keychain
    needs no signing identity.
  - Other secret names: `ASC_KEY_ID_SECRET=… ASC_ISSUER_ID_SECRET=…
    ASC_KEY_P8_SECRET=…`.
- **Build numbers:** App Store Connect gives each upload the next number
  (`manageAppVersionAndBuildNumber`), so the project's `CURRENT_PROJECT_VERSION`
  stays at 1. The marketing version is `MARKETING_VERSION` in
  `project.pbxproj`. Raise it for a release testers should see as new.
- **Without uploading:** `just native-testflight export` signs the app the same
  way and leaves the `.ipa` in jake-mini's
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

What isn't here yet: text selection in the terminal, and answering from a
notification.
