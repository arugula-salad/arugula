---
schema: 1
id: "aru-072"
title: "The project is the unit of platform sharing"
state: "decided"
area: "Federation and control"
source:
  via: "cli"
  issue: "arugula-salad/arugula#1123"
question: "What do platform objects (Cloud workspaces, team channels, cloud secrets, later issues, runs and deploys) belong to?"
options:
  - id: "a"
    label: "A project: org → team → project, owned by a team or an account; platform objects hang off it"
    how: "A projects table (prj_ ids), one default project per team and a personal one per account that has platform rows; a project_id on cloud_workspaces, chat_rooms and cloud_secrets. DMs and pane threads stay unattached: they're about people and machines."
    tradeoff: "A new noun and a migration; nothing reads it until later steps."
  - id: "b"
    label: "Keep keying rows by account, and team rooms by strings like team:<id>"
    how: "As today."
    tradeoff: "No place to share a set of platform objects with some of a team, and every feature invents its own scope."
  - id: "c"
    label: "The team is the unit"
    how: "Platform objects carry a team_id."
    tradeoff: "A team with two products can't separate them, and a solo account has no team."
choice:
  option: "a"
  reason: "Every later platform step (issues, runs, deploys, the hub/platform split) needs one owner for a set of platform objects; a project owned by a team or an account covers solo use and teams (Jake, 2026-10-20, #1122)."
rejected:
  - option: "b"
    why: "Account keys and scope strings are how 54 tables ended up without a shared noun."
  - option: "c"
    why: "Too coarse for a team with more than one thing, and absent for a solo account."
supersedes: []
evidence:
  - title: "#1122"
    url: "https://github.com/arugula-salad/arugula/issues/1122"
  - title: "#1125"
    url: "https://github.com/arugula-salad/arugula/issues/1125"
decided_by: "jhgaylor"
decided_on: "2026-10-20"
reviews: []
constrains:
  - "path:crates/control/src/db"
  - "path:crates/control/src/db.rs"
---

# The project is the unit of platform sharing

Platform objects belong to a **project** (Jake, 2026-10-20, #1122). The
hierarchy is org → team → project: a project belongs to exactly one team or
one account (`team_id` or `account_id`, never both), and a team or an account
has one default project. Cloud workspaces, team channels and cloud secrets
carry a `project_id`, and so will later platform objects (issues, runs,
deploys). A new team channel goes to its team's default project; a new Cloud
workspace or cloud secret goes to its owner's personal project until people
can choose.

DMs and pane or session threads don't hang off a project: they're about
people and machines, not about a thing being built. Machines, sessions and
panes stay on the machine plane ("Two planes, two trust rules") and are
shared as "One function says what an intent needs" says, not through
projects.

A team's signed roster stays the authority over who reaches a team's
machines. Control keeps `team_members`, a projection of each roster
rewritten in the same transaction as the roster, for platform queries; it's
never read for machine access.

Step 1 adds the rows and backfills them, and nothing reads `project_id` yet,
so behaviour doesn't change.
Where: `crates/platform/src/db/projects.rs` (`projects`, `team_members`),
`crates/platform/src/projects.rs`, and `project_id` on Cloud's and chat's
tables (`crates/platform/src/db/`).
From: #1122, M129 (#1125).
