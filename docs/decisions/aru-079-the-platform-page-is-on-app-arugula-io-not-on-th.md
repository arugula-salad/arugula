---
schema: 1
id: "aru-079"
title: "The platform page is on app.arugula.io, not on the keys' origin"
state: "decided"
area: "Clients"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1172"
question: "Where does the page for projects, chat and Cloud workspaces live, now that it's growing beside the page that holds device keys?"
options:
  - id: "a"
    label: "A page of its own on app.arugula.io, served through the hub; the shell on control.arugula.io keeps the keys and trust"
    how: "web/src splits into shell/, platform/ and shared/. The platform page is its own Vite build, embedded and served by the platform binary; the hub routes the app.arugula.io host to it. CI refuses any import from platform/ of key code, the daemon client, control's signing paths or the fleet."
    tradeoff: "Two origins, so signing in to the platform page is one hop through the hub, and the desktop app has to trust a second origin."
  - id: "b"
    label: "Keep one bundle on control.arugula.io and split only the code"
    how: "Folders and an import check, but the platform views keep running on the keys' origin."
    tradeoff: "Any script the platform views load can still use the device keys; the separation is only as strong as review."
  - id: "c"
    label: "A platform page on its own registrable domain"
    how: "A domain that isn't arugula.io."
    tradeoff: "Not same-site with control, so cookies and the silent sign-in hop get harder, for no gain over a sibling host."
choice:
  option: "a"
  reason: "Scripts on app.arugula.io can't use device keys at all, which is the limit docs/control-e2e.md admits today; same-site with control keeps sign-in to one silent hop; and serving it through the hub keeps step 2's rule that the platform has no public address (Jake, 2026-10-20, #1171). This overrides experimental's D10 claim on arugula.io."
rejected:
  - option: "b"
    why: "An import check without an origin boundary still lets platform code run beside the keys."
  - option: "c"
    why: "Cross-site cookies and sign-in for nothing a sibling host doesn't give."
supersedes: []
evidence:
  - title: "#1171"
    url: "https://github.com/arugula-salad/arugula/issues/1171"
  - title: "#1172"
    url: "https://github.com/arugula-salad/arugula/issues/1172"
  - title: "#1174"
    url: "https://github.com/arugula-salad/arugula/issues/1174"
  - title: "#1177"
    url: "https://github.com/arugula-salad/arugula/issues/1177"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:web/src/shell"
  - "path:web/src/platform"
  - "path:web/src/shared"
  - "path:web/vite.platform.config.ts"
  - "path:crates/platform"
---

# The platform page is on app.arugula.io, not on the keys' origin

The page that holds the team's work is a separate origin from the page that
holds the device keys (Jake, 2026-10-20, #1171):
- **The shell** stays on `control.arugula.io`, embedded by the daemon ("The
daemon serves the web client, on purpose", amended). It holds the device
keys and trust, and keeps terminals, blocks, the swarm, attention and pane
threads.
- **The platform page** is on `app.arugula.io`. It holds projects, chat and
Cloud workspaces at real URLs. The platform binary embeds and serves it,
and the hub routes the `app.arugula.io` host to the platform, so the
platform still has no public address ("Platform paths stay on
control.arugula.io forever, behind the hub's gateway"). Old platform paths
on `control.arugula.io` keep working for old daemons and apps.
- **The platform page never imports key code.** `web/src` is `shell/`,
`platform/` and `shared/`. CI refuses any import from `platform/`, directly
or through `shared/`, of the key code (`e2e/`, device keys, cert signing),
the daemon client, control's signing paths or the fleet, and `shared/`
imports neither side (M148). A bundle check proves the platform page's
build holds no key modules, and that the daemon's bundle holds no platform
modules (M155).

Scripts on `app.arugula.io` can't use device keys, even a modified client:
that closes the limit `docs/control-e2e.md` admits for chat and Cloud.
`app.arugula.io` is same-site with `control.arugula.io`, so signing in is
one silent hop ("The hub signs people in to the platform page"). This
overrides experimental's D10 claim on `arugula.io`.
Where: `web/src/shell/`, `web/src/platform/`, `web/src/shared/`, `web/vite.platform.config.ts`, `crates/platform/`.
From: #1171 (platform roadmap, step 3), M146 (#1172), M148 (#1174), M151 (#1177).
