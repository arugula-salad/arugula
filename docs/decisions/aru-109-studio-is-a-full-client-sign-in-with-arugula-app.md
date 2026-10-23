---
schema: 1
id: "aru-109"
title: "studio is a full client: Sign in with Arugula, apps by project"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1337"
question: "How much of the platform does studio take on: only the sign-in, or its accounts, projects, secrets and broker too?"
options:
  - id: "a"
    label: "A full client: Sign in with Arugula, app ownership as project membership, secrets and the Claude broker from the platform, beds retired"
    how: "The lobby signs people in through the hub and stores the numeric GitHub id, not only the login. An app's owners are its project's members, so an app can have more than one. Egress secret values come from the platform's project secrets, and the lobby's Claude calls go through the platform's broker. beds retires once step 5's workspaces can plant repos. studio's own passkeys stay as a fallback for a quarter."
    tradeoff: "studio gives up its own account system, app ownership and broker, and depends on the hub and the platform for each."
  - id: "b"
    label: "Share only the sign-in assertion"
    how: "studio trusts the hub's assertions, and keeps its own accounts, app ownership, secrets and broker."
    tradeoff: "A studio user is still a row of studio's own, an app still has one owner, and secrets and Claude spend stay in two places."
  - id: "c"
    label: "studio stays apart"
    how: "As today: passkeys and GitHub login only."
    tradeoff: "studio stays one of seven sign-ins."
choice:
  option: "a"
  reason: "One account, one project, one store and one view of Claude spend reach studio's apps too, and a teammate can open an app they didn't create (Jake, 2026-10-22, platform step 8, deciding Alex's part on Alex's behalf, pending Alex's review)."
rejected:
  - option: "b"
    why: "The fallback if Alex declines a full client in review; see the body."
  - option: "c"
    why: "One account per person."
supersedes: []
evidence:
  - title: "#1337"
    url: "https://github.com/arugula-salad/arugula/issues/1337"
  - title: "Arugula #22"
    url: "https://app.arugula.io/issues/iss_69d1c528120426c6b9a776435c9a10d6"
  - title: "Arugula #34"
    url: "https://app.arugula.io/issues/iss_508e52d043e9a4da4252a28c1e68c041"
decided_by: "jhgaylor"
decided_on: "2026-10-22"
reviews: []
constrains:
  - "path:crates/platform/src/projects.rs"
  - "path:crates/platform/src/secrets.rs"
  - "path:crates/platform/src/hosted/inference.rs"
---

# studio is a full client: Sign in with Arugula, apps by project

studio becomes a full client of the platform (Jake, 2026-10-22, platform
step 8, #1337):
- **"Sign in with Arugula"** in the lobby, through the hub ("The hub is an
  issuer: one identity for every product"). studio stores the numeric GitHub
  id, not only the login.
- **App ownership becomes project membership,** so an app can have more than
  one owner.
- **Egress secret values come from the platform's project secrets** ("The
  platform is the one store for users' project secrets; Infisical stays the
  operator's").
- **The lobby's Claude broker calls the platform** ("The Claude credential
  broker and its metering move to the platform").
- **beds retires,** once step 5's workspaces can plant repos.
- **studio's own passkeys stay as a fallback** for a quarter.

studio is Alex's repo. **Jake decided this on Alex's behalf,** pending Alex's
review of this record. The code there is proposed as PRs that Alex reviews
and merges.

**The fallback:** if Alex declines being a full client in review, studio
shares only the sign-in assertion (option b) and stays its own account
system. This record is then amended to say so, and the rest of studio's work
in step 8 goes back to Jake as a choice.
Where: studio (Arugula #34, M236, native), `crates/platform/src/projects.rs`, `crates/platform/src/secrets.rs`, `crates/platform/src/hosted/inference.rs`.
From: #1337 (platform step 8, tracked natively in Arugula #22).
