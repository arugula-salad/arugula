# The iPhone and iPad app

SwiftUI on the Rust core in `../core`, which holds signing in, the encrypted
channel to a machine and the terminal ([aru-132](../../docs/decisions/)). The core crosses
to Swift through a small C interface, `Arugula/arugula_core.h` and
`../core/src/ffi.rs`, mostly as JSON.

One app for both: on an iPhone each screen is one at a time, and on an iPad
a list sits beside what's open from it (a project, a chat room, a machine).

The tab bar is always there (#1262), with the web's five places (#1361):
**Overview · Boards · Docs · Machines · Chat**. On an iPad it's
`.sidebarAdaptable`: a sidebar in landscape that folds into the floating bar
in portrait. Every place is headed by the project's menu, **New**, the
**bell** (the Inbox) and **your picture** (your name, Settings, Send feedback,
the version and Sign out: `Arugula/Account.swift`), which fit on a 390pt
phone. Settings opens the web's page in a web view, its sign-in handed over
by control (`POST /api/handoff`, Arugula #1420). Issues aren't a place: a board's rows open their
issues, and the project's menu opens its list. Docs lists the project's Docs
with #1363; until then it says so.

The **Overview** (`Arugula/Home.swift`, #1255; Home until #1361) is the first
place: the current project's conversations, sorted by what needs you, as the desktop's Overview reads them.
It is the Overview's Project level (#1304, the web's #1287): a board's row
zooms to its **Board level** and a ticket there to its **Work level**
(`Arugula/Zoom.swift`), pushed on an iPhone and beside the Overview's list on an iPad;
Back or the path bar over each level zooms out. Each level has the web's
address under `arugula://`, which the app registers: `arugula://p/PROJECT`,
`…/board/BOARD`, `…/board/BOARD/card/ROW` and `…/work/SESSION` (a session's
key: `pane:MACHINE:PANE`, `hosted:SESSION` or the web's `MACHINE#SESSION`). A
link to one, the web's `https://…/p/…` from a Doc too, opens that level in its
project; a notification about a pane opens its card's Work level (or its own),
and one about an issue on a board, its card's.
The **Board level** (`Arugula/BoardLevel.swift`, #1306) shows the board's goal
and progress (All lanes opens its lanes), its lead's card with **Talk to the
lead** (that agent) and **Board thread** as two buttons (or, with no lead in a
team's project, a way to ask the chief of staff to start one), and its open
tickets as the board groups them, each with its PR or checks, a word (working,
in review, next, asks you) and the agents on it, whose names open them.
The **Work level** (`Arugula/WorkLevel.swift`, #1307) shows the card's
sessions live: a terminal is drawn by the core at the pane's own size, scaled
to fit (`aru_link_watch` of its tab, claiming nothing, so nobody's panes
resize), and a tap types into it; an agent block shows its screen as it goes
(`/api/panes/{id}/capture`) with a box that types to it; a hosted session its
last events and a reply in its thread. An agent's question, permission or
failure sits over the panes with its answers, as Needs you answers it. Up to
four panes side by side on an iPad, the rest in a strip; one at a time on an
iPhone. A viewer's panes are read, not typed into.
In a team's project it starts with the **chief of staff** (#1279): the
reader's staff room (`POST /api/chat/projects/{project}/staff`), which only
they and their own agents read, with the session that leads it.
Needs you merges every online machine's panes that ask or failed (a link to
each, as the web's fleet opens them), the boards' rows that ask you, fail
their checks or wait for review, and the hosted sessions that wait on you,
worst first. Then one row per open board (its lane bar, `done/total`, what's
going on and who leads it), which zooms to the board, whose Thread and
Lanes buttons open its thread and its lanes; then the channels and DMs. On an iPad the Overview is the
left column, and what it opens sits beside it. A notification opens its place:
a pane's on Machines, a room's on Chat, an issue's beside the Overview.

A row of Needs you is **answered where it is** (`Arugula/Answer.swift`, #1256),
and folds away with a word of what was done: a pane's permission (Allow,
Deny) or question (its choices) through its daemon's `POST
/api/attention/act` over the link, a hosted session's through control's
`POST /api/sessions/{ses}/actions`, a board's ask through its answer route.
A lead's or an agent's name opens **that agent** (#1305): on the Overview's board
rows, on **Boards** (where a lead shows its state dot, its session, a
teammate's with their name, and its machine) and over a board's thread or the
staff room, it opens the session's Work level, where you type to it; one on
a machine this device doesn't reach says where it runs. **Board thread** is
a button of its own, and a line under a thread or a peek says who you're
talking to and where a post goes. A lead's session is found by its
conversation's title, an agent block's title or its session's name on its
machine; Peek over the staff room or a board's thread opens it too.
Tapping a session, in Needs you or on a board's thread, opens its **peek**:
its last lines, a reply to its agent, Silence (the owner's: teammates'
agents can't mention it), Stop (asked first) and its terminal or thread. A
viewer of a machine's session reads it, with no buttons.

**New**, in every place's heading (`Arugula/New.swift`, #1257, #1361), tells
a board (its thread opens, and a post there reaches the session that leads it) or starts a **New idea**.
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

**What a passkey approves** (`Arugula/Approvals.swift`, #604): a deploy
waiting on you is first under Needs you (`GET /api/deploys/waiting`), and a
tapped deploy notification opens it over whatever's open. Its sheet shows
the project, what's deployed from where, who asked and the plan; the
project menu's **Runs** shows the project's standing grant for runs (and
nothing to allow on a project with no cloud workspaces). Each
approval is signed with a passkey of the account's, whose RP is control's
host, and the app isn't associated with it (no `webcredentials`), so it
never asks for one itself: **Approve or deny with passkey** (and **Allow
runs**, **Renew or revoke**) opens control's own dialog, `#deploy=…&app` or
`#run-grant=…&app`, in a browser sheet (`ASWebAuthenticationSession`,
sharing Safari's sign-in). There the page checks what's signed and asks for
the passkey; done or closed, it goes to `arugula://back`, the sheet closes,
and the app reads the deploy or the grant again. Owner changes stay on the
web: the app has no team screen.

A Doc linked in a message is a card that opens the Doc's page from control
(`/embed/p/:project/docs/:doc`) in a web view (`DocViews.swift`). Control
sets no cookie there: the app puts its own session's cookie in the view's
store, so the page reads and comments as the app, a native session. The
page's links to a board, an issue, a room or another Doc come back as intents
on `window.webkit.messageHandlers.arugula` and open in the app. On an iPhone
the Doc is over everything; on an iPad it's a third column beside the
conversation, the Overview's list folded away until Close. Debug builds allow plain
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
  board the claim made on the Overview with its row's question under Needs you, opens
  the board's thread, posts there and opens its lanes; peeks at a stand-in
  agent, allows its permission prompt and answers its question and the
  board's from the Overview (the claim checks the agent went on); tells the board and
  starts a New idea (testnet has no hosted sessions, so it runs on
  box-systemd, which the claim checks), then reads projects,
  posts in chat and runs a command in a relayed terminal. A deploy the claim
  planned waits under Needs you: the test opens it and hands over the
  address Approve opens, the claim approves it with the account's passkey
  (as control's dialog would, in the web sheet a test can't drive), and
  the app reads it approved; then the project's Runs shows its grant.
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
  question), a chat room, an issue, or a deploy waiting for approval.

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
