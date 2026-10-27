---
schema: 1
id: "aru-117"
title: "Four Labs flags stay, renamed; the rest graduate or retire"
state: "decided"
area: "Build and release"
source:
  via: "cli"
question: "Which Labs features stay behind a flag, what are those flags called, and what becomes of the others and of their old names?"
options:
  - id: "a"
    label: "Keep four flags under clearer names, old names read forever; graduate chat, docs, guest ssh and the swarm's views; retire studio's connector once the platform covers it"
    how: "sandbox-providers, chant-workspaces, additional-forges and agent-delegation stay in Labs. vms, workspaces, forges and agents are aliases of them wherever a name comes in, in one table, and are listed nowhere. Chat and Docs are for everyone; guest ssh is a sharing setting, off by default; the swarm's views are a preference under Theme. The `studio` flag stays until the platform covers what its connector does; chat's legacy thread import goes with no replacement. Each flag that leaves is a RETIRED name (aru-066)."
    tradeoff: "Every place a flag name comes in has to resolve an alias, forever, and control has to learn the renames, which it didn't know before (aru-068)."
  - id: "b"
    label: "Rename the four flags with no aliases"
    how: "The new names replace the old ones; an old name is unknown, like any name that isn't a flag."
    tradeoff: "A machine with vms on in flags.json, or a control that set it for an account or team, silently loses the feature on update."
  - id: "c"
    label: "Keep all eight flags as they are"
    how: "Nothing graduates, nothing is renamed."
    tradeoff: "Chat and Docs stay hidden from people who have every reason to use them, the names say little (vms, agents), and studio's second login lingers beside the platform's."
choice:
  option: "a"
  reason: "Jake's Labs flags rethink, 2026-10-26: Labs is for what a stranger can't use yet, so what works for everyone leaves it, and what stays says what it is. No machine loses a setting to a rename."
rejected:
  - option: "b"
    why: "A rename must not turn anything off: the old names stay readable forever."
  - option: "c"
    why: "Chat, Docs, guest ssh and the swarm's views are finished features; studio's connector is to be superseded by the platform's app access."
supersedes:
  - decision: "aru-064"
evidence:
  - title: "PR #1543 (swarm-themes graduates)"
    url: "https://github.com/arugula-salad/arugula/pull/1543"
decided_by: "jhgaylor"
decided_on: "2026-10-26"
reviews: []
constrains:
  - "aru-064"
  - "aru-066"
  - "aru-068"
  - "path:crates/proto/src/flags.rs"
  - "path:crates/control-wire/src/team.rs"
  - "path:crates/control/src/labs_flags.rs"
  - "path:crates/control/src/db.rs"
  - "path:web/src/shell/client.ts"
  - "path:web/src/shell/trust/teams.tsx"
  - "path:docs/labs.md"
---

# Four Labs flags stay, renamed; the rest graduate or retire

Jake's Labs flags rethink (2026-10-26). Labs is for what a stranger can't
use yet (it needs our own setup, or isn't finished), so what works for
everyone leaves it, and the flags that stay are named for what they turn
on. This replaces the list of flags in "A flag for each Labs feature, and
Developer settings to turn them on" (aru-064); the rest of it (a flag per
feature, `labs` meaning all of them, Developer settings unlocked first)
stands, as do "Each flag gates its feature" (aru-065), "A retired flag is
a name that isn't one" (aru-066) and "Control pushes Labs flags; the
machine overrides" (aru-068).

**Four flags stay, under new names:**
- **`sandbox-providers`** (was `vms`), *External sandbox providers*:
  daemon-managed VMs through the machine's own provider (VM tabs and
  panes, machines, the sandboxes page). The platform's hosted (cloud)
  workspaces are a separate thing, and need no flag.
- **`chant-workspaces`** (was `workspaces`), *Chant workspaces*: chant's
  workspace inspection, relationships and gates.
- **`additional-forges`** (was `forges`), *GitLab and Forgejo*: stays in
  Labs until their integration with platform projects and workflows is
  coherent. GitHub's blocks are always on.
- **`agent-delegation`** (was `agents`), *Local agent delegation*:
  recipes, the team's catalog and A2A tasks. Still local-only (only the
  machine's `flags.json` sets it) and still offering nothing until its
  owner offers a recipe.

**Old names are aliases, forever.** One table, `flags::ALIASES`, maps each
old name to the new. An old name means its flag wherever a name comes in:
in `flags.json`, in what control pushes (aru-068), in `PUT
/api/flags/<old>` and in `arugulad flags <old> on|off|unset`. So no
machine loses a setting.
- Where both names are said, the new name's word wins.
- Setting a flag writes its new name; an old name the file already has is
  written too, in step, so an older daemon reading the same file agrees. A
  new file gets the new name only. Taking it back (`unset`) takes both.
- Only the new names are listed: Developer settings, `arugulad flags`,
  docs, both `--help`s, `arugula mcp` and `FLAGS` in `proto.gen.ts`.
- **Control learns the renames** (until now it knew no flag's name):
  `wire::FLAG_ALIASES`, which a daemon test checks is `flags::ALIASES`.
  Control stores the new name (rows under an old one move at startup,
  unless the owner set the new one too), takes an old name from an older
  page, and sends a machine each renamed flag under both names, so a daemon
  from before the rename still hears it.
- **The web page asks by the new name** and reads an old one from a daemon
  from before the rename (`FLAG_ALIASES` in `proto.gen.ts`), since control
  serves the page to every daemon version.

**Four graduate**, each a `RETIRED` name (aru-066) so a file or a push
that still names it changes nothing:
- **`chat`**: Chat is the platform's, for everyone signed in to control;
  control's own chat setting for the account or team decides it, not a
  machine flag. **Chat's legacy thread import is removed**, not moved:
  nothing brings a machine's old `threads/` into control any more.
- **`docs`**: Doc blocks, the `doc` tool and the Doc library are on
  everywhere. Sharing a doc still asks the user.
- **`guest-ssh`**: guest ssh access is a setting of the machine's sharing,
  off by default, set only by its owner (local-only, as the flag was). A
  machine that had the flag on keeps it on.
- **`swarm-themes`**: the swarm's city, hive and timeline views are
  everyone's, picked under the swarm's Theme and kept in the browser.

**Studio's connector retires later.** The daemon's own studio login and
personal-access-token connector (linking a studio, `/api/studio…`, its
token) is to go, with the `studio` flag (then `RETIRED`), and opening apps
beside terminals is to go through the platform's app access. It stays, behind
`studio`, until the platform covers what the connector's hud follower does
(a box agent's asks, hud gates as attention, sending prompts, Records) and
signing in to an app in its frame is checked (Jake, 2026-10-26: native #206
held).

Where: `crates/proto/src/flags.rs` (`FLAGS`, `ALIASES`, `canonical`, `lookup`, `RETIRED`), `crates/control-wire/src/team.rs` (`FLAG_ALIASES`, `LOCAL_ONLY_FLAGS`), `crates/control/src/labs_flags.rs` and `db.rs` (`migrate`), `crates/daemon/src/ops/flags.rs`, `crates/daemon/src/main.rs` (`flags_cli`), `web/src/shell/client.ts` (`flag`), [docs/labs.md](../labs.md).
From: Jake's Labs flags rethink (2026-10-26), native Arugula #202 to #207; swarm-themes in PR #1543.
