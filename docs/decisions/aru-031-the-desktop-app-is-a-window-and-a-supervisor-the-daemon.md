---
schema: 1
id: "aru-031"
title: "The desktop app is a window and a supervisor; the daemon stays a service"
state: "decided"
area: "Clients"
source:
  via: "cli"
question: "The desktop app is a window and a supervisor; the daemon stays a service"
options:
  - id: "a"
    label: "The desktop app is a window and a supervisor; the daemon stays a service"
    how: "As the body below says."
    tradeoff: "Not recorded: DECISIONS.md kept what was chosen and why, not the options weighed."
choice:
  option: "a"
  reason: "Given in the body below, as DECISIONS.md had it."
rejected: []
supersedes: []
evidence:
  - title: "Desktop track"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#desktop-track-s25-m46m48-added-2026-10-04"
  - title: "S26"
    url: "https://github.com/arugula-salad/arugula/blob/main/docs/plan-archive.md#s26-how-native-can-it-get-141"
  - title: "#317"
    url: "https://github.com/arugula-salad/arugula/issues/317"
  - title: "#390"
    url: "https://github.com/arugula-salad/arugula/issues/390"
  - title: "#392"
    url: "https://github.com/arugula-salad/arugula/issues/392"
  - title: "#550"
    url: "https://github.com/arugula-salad/arugula/issues/550"
  - title: "#561"
    url: "https://github.com/arugula-salad/arugula/issues/561"
decided_by: "jhgaylor"
decided_on: "2026-10-06"
reviews: []
constrains:
  - "path:crates/desktop/src/main.rs"
  - "path:crates/desktop/src/service.rs"
  - "path:crates/desktop/src/compat.rs"
  - "path:crates/desktop/src/updates.rs"
  - "path:crates/daemon/src/install.rs"
---

# The desktop app is a window and a supervisor; the daemon stays a service

The app (Tauri 2) loads the UI from the local daemon, bundles `illogicald` and
`illogical`, and registers the daemon as a login service (SMAppService on
macOS). Sessions outlive the window: if the app owned the PTYs, quitting it
would end every pane. `desktop` depends only on `proto`.
The app never replaces a daemon that's there (#392): the bundled copy only
installs one on a machine with none, and a daemon updates itself (#391). At
launch the app checks the daemon's protocol number against its range (#390),
and a daemon that reports none must be 0.19.0 or newer, the titlebar's
release (#317). Otherwise its window stays on the app's setup page, which
says which side is behind and offers that side's update; the app never shows
a daemon's page it doesn't match.
An app update that replaces the app in place (macOS, the AppImage) restarts
the app by itself once nobody's using it (#561): no window of ours focused
and, on macOS, no input anywhere for five minutes, and no dialog or file
picker open. Running on from a replaced bundle breaks the
macOS services the app reaches through it (the file picker, #556), and the
panes are the daemon's, so a restart loses nothing; the windows come back
on their pages without taking focus. The tray's *Restart to update* stays
for doing it sooner. Windows' installer closes the app as it installs, so
there's nothing to wait for there.
On macOS the app's launch agent runs the daemon in the bundle, from a plist
the bundle's signature covers, so `arugulad install -- FLAGS` can't write
its flags into that plist as it does into its own plist or unit: it keeps
them in the state dir's `daemon-args.json`, and a daemon that agent starts
with no flags reads them from there (#550).
Where: `crates/desktop/src/main.rs`, `crates/desktop/src/service.rs`,
`crates/desktop/src/compat.rs`, `crates/desktop/src/updates.rs`, `crates/daemon/src/install.rs`.
From: [Desktop track](docs/plan-archive.md#desktop-track-s25-m46m48-added-2026-10-04), [S26](docs/plan-archive.md#s26-how-native-can-it-get-141), #317, #390, #392, #550, #561.
