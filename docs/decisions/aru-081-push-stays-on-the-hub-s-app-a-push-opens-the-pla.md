---
schema: 1
id: "aru-081"
title: "Push stays on the hub's app; a push opens the platform page"
state: "decided"
area: "Clients"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1172"
question: "Which installed app and service worker show chat pushes once chat lives on app.arugula.io?"
options:
  - id: "a"
    label: "The hub's app: one install, one permission, and tapping a push opens the platform page"
    how: "control.arugula.io's service worker (web/src/sw.ts) keeps showing chat pushes and opens the app.arugula.io URL for the room or message. The platform page gets a manifest and icons but no push subscription of its own."
    tradeoff: "On a phone the platform page opens in the installed app's in-app browser or a Custom Tab, not as its own installed app; if that can't land someone in the room usably on iOS, chat is framed into the hub's app on the phone only."
  - id: "b"
    label: "A second install and subscription for app.arugula.io"
    how: "The platform page registers its own service worker and asks for notifications."
    tradeoff: "Two installs and two permission prompts, and pushes split by origin."
choice:
  option: "a"
  reason: "One install and one permission prompt; nobody re-subscribes; proven on a real iPhone and Android phone before it ships (Jake, 2026-10-20, #1171)."
rejected:
  - option: "b"
    why: "A second install and prompt for the same people and the same pushes."
supersedes: []
evidence:
  - title: "#1171"
    url: "https://github.com/arugula-salad/arugula/issues/1171"
  - title: "#1172"
    url: "https://github.com/arugula-salad/arugula/issues/1172"
  - title: "#1182"
    url: "https://github.com/arugula-salad/arugula/issues/1182"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:web/src/sw.ts"
  - "path:crates/control/src/push.rs"
---

# Push stays on the hub's app; a push opens the platform page

Push stays on the hub's app (Jake, 2026-10-20, #1171). There is one install
(control's) and one permission. The hub's service worker (`web/src/sw.ts`)
keeps showing chat pushes, and tapping one opens the `app.arugula.io` URL
for that room or message, signed in through the hub's silent hop ("The hub
signs people in to the platform page"). On iOS the installed app opens it
in its in-app browser; on Android, in a Custom Tab or the browser. The
platform page gets a manifest and icons, and no push subscription of its
own.

It ships only after real iPhone and Android runs (M156). If iOS can't land
the person in the room, signed in and usable, the fallback is chat framed
into the hub's app on the phone only, decided after a report with
screenshots.
Where: `web/src/sw.ts`, `crates/control/src/push.rs`.
From: #1171, M146 (#1172), M156 (#1182).
