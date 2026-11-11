---
schema: 1
id: "aru-140"
title: "An owner has many projects; channels belong to the team; the default project is a setting"
state: "proposed"
area: "Federation and control"
source:
  via: "cli"
question: "When a team, or an account for Personal, can have more than one project, what owns what: the project, its team's channels, and the project an agent uses when nothing names one?"
options:
  - id: "a"
    label: "An owner has many projects; channels belong to the team; the default project is a setting"
    how: "A project stays owned by one team or one account (aru-072), and an owner has one or more. One Owner type and one access check decide a person's role in a project: their team role, or Owner of their own account's projects. Team channels carry team_id; a channel's project_id becomes an optional tag, and threads.* powers over a team's channels are granted on the team. is_default goes: each owner has a default_project setting, where an agent's issues and Docs go when no session, block or linked repo names a project. A repo links to exactly one project. A project can move to another owner; its issues, numbers, Docs, boards and links go with it."
    tradeoff: "Every reader of is_default, and every place that decides a role from team_id or account_id, has to move onto the shared check and the setting. Chat's reach and agent grants change shape."
  - id: "b"
    label: "Personal becomes a team with one member"
    how: "The hub keeps a team row of kind personal for each account, with no signed roster, and every project has a team_id."
    tradeoff: "One owner kind, but the personal kind reaches every hub path that lists or acts on teams: about 30 callers of teams_of, invites, rosters, lock, machine moves and Labs team flags. Each has to skip or refuse it."
  - id: "c"
    label: "Channels stay keyed by the team's default project"
    how: "A team's channels keep project_id set to its first project, and a grant on that project reaches them."
    tradeoff: "A team's chat looks like one project's, and a grant on any other project of the team reaches none of its channels."
choice:
  option: "a"
  reason: "People switch between projects, while people, chat and most settings belong to the team (Aaron, 2026-11-11, the projects RFC, Doc dcc5ded86b870131). Keeping account ownership leaves aru-072 as it is and the hub unchanged; one access check removes about 16 copies of the team-or-account fork. The Boards and Docs leads asked for a default that answers when nothing else does, and for one resolver shared by issues and Docs."
rejected:
  - option: "b"
    why: "A team with no roster, no chat and no invites would leak into roster and trust code to save a fork that one access check removes anyway."
  - option: "c"
    why: "Chat is the team's, across its projects; tying it to one project is what made team and project read as the same thing."
supersedes: []
evidence:
  - title: "Projects as the main boundary (RFC), Doc dcc5ded86b870131"
  - title: "Arugula #888 (this record)"
  - title: "Board: Projects as the main boundary (brd_dc9286c740de6f169f9f85056d9b6266)"
decided_by: "alecraso"
decided_on: "2026-11-11"
reviews: []
constrains:
  - "aru-072"
  - "aru-088"
  - "aru-139"
  - "path:crates/control-wire/src/projects.rs"
  - "path:crates/control-wire/src/issues.rs"
  - "path:crates/control-wire/src/chat.rs"
  - "path:docs/data-model.md"
---

# An owner has many projects; channels belong to the team; the default project is a setting

Every team and every account has had exactly one project, and nothing made a
second, so a team and its project read as one thing (aru-072, aru-134). The
projects RFC (Doc dcc5ded86b870131) makes the project the thing a person is
in and switches between. Proposed (Aaron, 2026-11-11), for Jake's review:

- **An owner has many projects.** A project is still owned by one team or
  one account (aru-072 stands), and Personal stays account-owned. One `Owner`
  type and one access check decide a person's role in a project: their team
  role, or Owner of their own account's. Access is inherited from the team;
  a per-project list can come later.
- **Channels belong to the team.** A team channel carries `team_id`; its
  `project_id` becomes an optional tag. `threads.*` powers over a team's
  channels are granted on the team, so reaching them no longer depends on
  which project is the default. Object threads stay in their project. DMs
  are unchanged. Personal has no channels.
- **The default project is a setting.** `is_default` goes. Each owner has a
  `default_project`, where an agent's issues and Docs go when no session,
  block or linked repo names a project. The 2026-11-05 rule (a team's
  existing work and channels stay where they are) becomes its backfill.
- **A repo links to exactly one project**, as `linked_repos` already has it:
  close-on-merge, holds and auto-attach depend on it.
- **A project can move to another owner**, Personal to a team or back, with
  its issues, numbers, Docs, boards and links. Objects still don't move
  between projects.
- **One resolver** answers "which project?" for issues, boards and Docs:
  the named project, the block's issue, the session's project, the clone's
  linked repo, then the owner's `default_project`. Docs never get one of
  their own.

Not built yet: see the board "Projects as the main boundary" (Arugula #888
and the rows around it). Settings at owner and project scope
(`scoped_settings`) and the session's project pointer are in the RFC and its
tickets, not in this record.

Where: the platform's projects, chat and grants tables and their access
checks (now in arugula-salad/cloud), and `crates/control-wire/src/projects.rs`,
`issues.rs` and `chat.rs` here.
