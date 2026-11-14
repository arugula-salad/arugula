# The data model

What Arugula's nouns mean, where each one's truth lives, and who may change it.
Read this before adding a noun or reusing a word. Why it's this way is in
[aru-134](decisions/aru-134-one-data-model-what-each-noun-means-and-where-its-truth-lives.md).

Truth lives in one of three places:

- **The machine:** files under a daemon's state directory. Terminal content
  stays here.
- **The hub:** control's accounts, devices, rosters and routing.
- **The platform:** what has to exist while every machine is asleep. The tables
  are listed in [crates/control/tables.toml](../crates/control/tables.toml),
  with what each one holds.

The code doesn't match all of this yet. [What's not true yet](#whats-not-true-yet)
lists the differences.

## People and access

| Noun | What it is | Truth | ID |
|---|---|---|---|
| **Account** | A person's identity, with a root device | Hub `accounts` | 16 hex |
| **Device** | Keys and a certificate: browser, CLI, daemon, app or recovery | Hub `devices`, but each daemon decides for itself whether to trust one | 16 hex, a hash of its keys |
| **Team** | People with a signed roster | Hub `teams` | 16 characters |
| **Roster** | A team's signed, versioned list of members and roles | Hub `rosters`, checked again by each machine. `team_members` is a copy that's never read for machine access | Team + version |
| **Role** | Owner, editor or viewer | The roster, or a session's share in `acl.json` | The word |
| **Guest** | Someone with access who isn't a member | The machine | Depends on the kind |
| **Sign-in session** | A browser or app signed in to control, or to the platform's page | Hub `sessions`; `app_sessions` for the platform's page, each made from one in `sessions` and ending with it | A hash of its token |

**Grant** is five things: a person's role on a session, what a machine's agents
may do in a project, the minutes a guest may type into one pane, the standing
run grant and the delegation. Each has a word of its own, the same in the docs,
the CLI's help, the dialogs and Settings. "Grant" itself stays only as the
`arugula grants` command and the platform table's name, and a power's name,
like `issues.write`, doesn't change:

| Say | For | Truth | ID |
|---|---|---|---|
| **Share** | A person's role on a session, or a read-only link to one | `<state>/acl.json` | The principal; a link's key |
| **Team role**, **project role** | The roster's word: the same roster, seen from a machine or from the platform | The roster, checked by each machine; the platform reads the hub's copy | The word |
| **Power** | What a machine's agents, or an agent token, may do in a project or your DMs, like `issues.write` | Platform `grants` | `grt_`; a token's, `agt_` |
| **Trust** | The minutes a guest may type into one pane | The machine, in memory | Pane + principal |
| **Allow** | What a project owner signs for: the delegation, so control may enroll hosted machines into the project, and the standing run grant, so the platform may start its runs with every device asleep | Hub `project_delegations`; platform `run_grants`, signed with a passkey | The project |
| **Guest** | A link guest, an SSH guest or a tailnet guest: someone with access who isn't a member | The machine | Depends on the kind |

So a share is the machine's word about a person, a power is the platform's word
about an agent, and neither reaches further than the other side would let it: an
agent's power works only while its owner still reads the scope, and a hosted
machine's people are the project's people. An owner's "owner" is one of four
too: of the machine, of a session (a share that renames, closes and shares it,
and nothing else), of a team, or of a project, which is the team's owner seen
from the platform.

A product a person links, such as studio's lobby, holds a **consent**, which
isn't one of the five: the person's yes on the sign-in consent screen is in
the hub's `issuer_grants`, and what their account lets the product see, call
or send is in the platform's `client_consents`, `inference_consents` and
`client_secret_grants`.

Org isn't built. Billing keys on an owner, with no org above it.

## Machines and compute

A **machine** runs `arugulad`. There are three kinds that control knows about:

| Kind | What it is | Identified in |
|---|---|---|
| **Personal** | Joined to an account | `daemons`, with no team; `control.json` |
| **Team** | Joined to a team; a signed move shifts a machine between personal and team | `daemons.team`; `control.json` |
| **Hosted** | Runs inside a Cloud workspace, for people, a deploy or a snapshot; reached only through control's relay | `hosted_machines`; `control.json` |

In the daemon the kind is one type, `MachineKind` in
`crates/daemon/src/control.rs`: personal, team (the pin and the roster it
checked), or hosted (the delegation, its purpose, and the project's team when a
team owns it). `control.json` keeps the flat shape every release has written,
so a daemon from before reads a file this one saved
([aru-136](decisions/aru-136-a-machine-s-kind-is-one-type-control-json-keeps-its-flat-shape.md)).

A daemon can also run without joining control at all, known only by its local
owner, and other daemons can list it by name in `hosts.json`.

A **Cloud workspace** (`hw_`) is a project's durable object: its spec, state and
snapshot. A **hosted machine** is what currently runs inside it, with its own keys
and ID. A rebuild replaces the machine and keeps the workspace. Say "Cloud
workspace" to people and "hosted machine" for the daemon inside. Don't say "box"
for either, because it also means hud and studio app boxes and a host reached
over ssh ([Don't say](#dont-say)).

Hosted machines stay out of the `daemons` table on purpose, so that what acts on
ordinary machines (moves, shares, powers) can't reach them.

The platform's `hosted_workspaces` also holds the workspaces deploys run on
and the ones that build snapshots, told apart by `purpose`: `workspace` for
people, `deploy` or `snapshot`. No person reaches a deploy's or a
snapshot's machine.

The hub decides who reaches a hosted machine by its project's owner, the
account or team its certificate names, not by the project
([crates/control/src/hosted.rs](../crates/control/src/hosted.rs), `role` and
`may_reach`): each member of a team reaches the hosted machines of every
project the team owns, and only its owners while the team is locked. That's right while a project's people are its team's.
A project with fewer people than its team needs this changed first.

An owner who removes a hosted machine at the hub fails its Cloud workspace.
The hub keeps the removal in `untold_hosted_removals` until the platform
takes it into `hosted_removed_machines`. The platform then fails the
workspace and pauses it, fails its runs, and stops waking its agent
sessions, which wait for a rebuild. Only the hub's word does this: a
machine missing from the hub's list changes nothing.

Other things called a workspace aren't this: a **chant workspace** is a Labs block
over a repo directory, and the **shell's workspace** is its layout.

## Work on a machine

| Word | What it means | Truth | ID |
|---|---|---|---|
| **Session** | A group of tabs on a machine, and the unit of sharing. It's in one project, or none: inferred from its first pane's linked repo, which it follows when that repo moves to another project, or set by hand, which nothing moves. A pane's `project` is its git repo, not this | `layout.json` | `$N` |
| **Tab** | A tab in a session | `layout.json` | `@N` |
| **Pane** | A process on a PTY, or any leaf of a tab | `layout.json`, `blocks/<id>/` | `%N` |
| **Block** | A typed leaf: agent, browser, Doc, editor and so on | `layout.json`, `blocks/<id>/` | `%N`, the same range as panes |
| **Agent** | A live agent in a pane or agent block, addressed as `machine/name` | Worked out live | Its title's slug |
| **Agent session** | Work the platform dispatches to a hosted machine | Platform `agent_sessions`; the machine keeps a ledger of it | `ses_` |
| **Conversation** | A Claude Code conversation on a machine | Claude Code's files in `~/.claude/` | Claude Code's |
| **Run** | A project's run on a hosted machine. Not the same as running a command in a pane | Platform `runs`; the machine keeps a ledger | `run_` |
| **Deploy** | A plan, its approval and its apply | Platform `deploys` | `dep_` |

So "session" alone means the group of tabs. `ses_` is always "agent session",
Claude Code's own session is a conversation, and a sign-in session is the
hub's (above).

A **card** is something waiting on a person. On a machine it's a block's
`Reason` ([crates/proto/src/lib.rs](../crates/proto/src/lib.rs)), shown in
its pane, on the swarm and on the phone: an agent's question or permission
(an `Ask`, in [crates/proto/src/ask.rs](../crates/proto/src/ask.rs)), a
diff, or a gate. An agent's invite and its Doc share are an `Ask` too,
raised on the invite block or the Doc block, never on the agent's pane. A
deploy waiting for approval is the platform's card, in its inbox
([web/src/platform/pages/inbox.tsx](../web/src/platform/pages/inbox.tsx)). A
**session card** is the chat message that shows an agent session's state
(`SessionCard` in
[crates/control-wire/src/sessions.rs](../crates/control-wire/src/sessions.rs)).
Chat's link previews, A2A agent cards and boxes on a page aren't cards in
this sense. Don't give the word another meaning.

## Chat

Chat's truth is on the platform. The daemon keeps only routing state. Who may
read a pane's or a session's thread is the machine's to say: it sends its
lists, and the platform keeps the latest in `chat_readers`. A closed pane's or
session's list only narrows: someone who loses access to it is off it for
good, and a share given after it closed adds nobody. A team's lock isn't a
loss: a member kept out only by it can't read the thread while the lock
lasts, and reads it again when the lock lifts (
[crates/mux/src/mux/readers.rs](../crates/mux/src/mux/readers.rs)).

| Word | What it is | ID |
|---|---|---|
| **Room** | Any place people chat: every `chat_rooms` row | Depends on the kind |
| **Channel** | A team's room | `ch_` |
| **DM** | A room for up to 8 people | `dm_` |
| **Object thread** | A room on an issue, PR, workspace, run, agent session, board or Doc | `ob_` |
| **Pane thread**, **session thread** | A room about one pane, or about a session (a group of tabs), on a machine | `th_` |
| **Reply thread** | A message and its replies, in any room. Who follows one is in `chat_thread_reads`, and `read_thread`'s `thread` names one by its first message | Room + seq |
| **Mention** | Naming a person, machine or agent in a message | Room + seq + machine |

"Thread" alone is a room: a pane thread, session thread or object thread, as
the `read_thread` and `post_thread` tools and the `threads.read` power mean
it. Say "reply thread" for a message and its replies.

## What a project holds

A **project** (`prj_`) is what platform objects belong to. It belongs to one
owner: a team, or Personal, a person's own account. An owner has one or more
projects ([aru-140](decisions/aru-140-an-owner-has-many-projects-channels-belong-to-the-team.md)).
Membership comes from the owner: a team's roster, or Personal's one person.

A project can be renamed, archived and moved to another owner
(`ProjectPatch` in
[crates/control-wire/src/projects.rs](../crates/control-wire/src/projects.rs)).
Anyone who may write it renames it; only its owner archives or moves it. A
move takes its issues, numbers, Docs, boards, linked repos and workspaces
with it, and revokes every grant over it, machines' and agent tokens'.
Objects still don't move between projects.

Settings come in layers: a project's override, else its owner's default,
else the built-in default. The platform keeps them in `scoped_settings`, one
row per layer and key. The keys are `issue_home`, `agents_model`,
`stall_minutes`, `agents_workspace` (a project's only) and
`default_project` (an owner's only).

Channels belong to their team (`chat_rooms.team_id`), not to a project, and a
`threads.*` grant can be over a team, which reaches its channels
(`GrantScope::Team` in
[crates/control-wire/src/chat.rs](../crates/control-wire/src/chat.rs)).
Object threads stay in their project, and Personal has no channels.

A session (a group of tabs) carries a project too, in `layout.json` (above).

One resolver picks the current project for issues, boards and Docs. In
order: the one named (`explicit`), the block's issue's (`block`), the
session's (`session`), the clone's linked repo's (`repo`), then the owner's
`default_project` (`default`). Its answer says which `rule` decided
(`IssueHome` in
[crates/control-wire/src/issues.rs](../crates/control-wire/src/issues.rs)).
Docs never get a resolver of their own.

| Noun | What it is | Truth | ID |
|---|---|---|---|
| **Linked repo** | A GitHub repo tied to one project | Platform `linked_repos` | GitHub's |
| **Native issue** | A project's own numbered issue | Platform `issues` | `iss_` + number |
| **Forge item** | A mirrored PR, GitHub issue or Plane item | GitHub or Plane; `forge_items` is a mirror | `fi_` |
| **Work item** | One piece of work behind a native issue, the GitHub issue it was exported to, or a Plane item. A board holds one row for it, whichever of them was added | Platform `work_items` and `work_item_refs` | `wi_` |
| **Board** | Rows of work with a goal | Platform `boards` | `brd_` |
| **Row** | One piece of work on a board | Platform `board_entries` | `row_` |
| **Group** | A named section of a board | Platform `board_entries` | `zon_` |
| **Lane** | Next, Doing, Review or Done, worked out from the work, with a person's override | Derived on read | The word |

Membership and roles alone decide who reads and writes a project's native
issues and Docs: a personal project's account, and a team's roster, where
owners and editors write and viewers read. Anyone else gets 404. A person's
chat setting doesn't enter into it, and nor does it for an agent acting for
them, which also needs its own power over the project. The `chat` setting
stays in the hub, because machines need it, and it still decides chat
itself: an issue's comments are its object thread, so they come with chat.

### Forge items: two read paths

GitHub or Plane is the record, and the platform's mirror is a copy of it. A
PR or issue block on a machine reads one of two ways, on purpose
([aru-087](decisions/aru-087-the-daemon-s-forge-reader-stays-for-unlinked-rep.md)):
the daemon's own reader stays for what the platform doesn't mirror. Which
path a block is on decides where each of its parts comes from:

| | Pinned block | Unpinned block |
|---|---|---|
| **When** | A github.com repo linked to a project the machine's owner is a member of, on a machine joined to control | Everything else: an unlinked repo, Forgejo, GitLab, a machine on a tailnet without control |
| **The item, its checks and events** | The mirror (`forge_items`, `forge_checks`, `forge_events`), read as the machine for its owner, through the viewer check | The forge, with the person's own `gh`, `tea` or `glab` |
| **Its text** (description, comments, reviews) | The platform's content read: GitHub's through the App, or `forge_content` where the project opted in; the block keeps it between changes | The forge |
| **Who "you" are** | The owner's GitHub login as the platform has it, without teams | The forge's word, with teams |
| **Waiting drafts** (comments, reviews) | Platform `drafts` (`dft_`): the block's go up once, and it shows the platform's | The block's config in `layout.json` |
| **Merge, rerun and live updates** | The block, in `layout.json` | The same |
| **Sends** | The person's `gh` from the block, with the platform told first; from the phone, a one-use token of theirs through the platform | The person's CLI |
| **Diff and checkout** | The person's git | The same |
| **The pin** `{kind, global_id}` | `layout.json` | None |
| **The block's log** | `blocks/%N/` on the machine | The same |
| **Attention** | The block's rules over the mirror's model; the platform's inbox works out its own from the same rows | The block's rules |

A block switches paths on what the platform says, and keeps the pin in its
config across an outage so the platform is asked first on every read:

- **Not linked, or no item the owner may read** (a 404): the pin goes and
  the block reads GitHub.
- **Unreachable:** what the platform said last stays, marked stale, for 10
  minutes; then GitHub until it answers. After a restart, GitHub at once:
  a block is never blank.
- **Refused** (a 400 or 409): GitHub at once.

Every fallback reads with the person's own GitHub login, so it shows what
GitHub lets that person read, which is what the viewer check asks GitHub
too; the check's other half, project membership, is what makes the repo
linked for them, and a block on a repo that isn't is an unpinned block
like any other.

A native issue block (`iss_`) is a third thing: a pin onto the platform's
`issues` row, read with the machine's `threads.read` power, never from a
forge.

About once a day for each linked repo, the mirror reads again the rows
GitHub no longer lists as open, and deletes one only when GitHub plainly
says it's gone or moved. Its `forge_content` goes with it.

### Drafts

A **draft** is a write an agent made that waits for a person to send it. There
are five kinds:

| Kind | What it is | Truth | ID |
|---|---|---|---|
| **Block draft** | A comment or review on an unpinned PR or issue block | The block's config in `layout.json` | The block's |
| **Platform draft** | A comment, review or new issue for a linked repo | Platform `drafts` | `dft_` |
| **The session's PR** | An agent session's pull request, as its asker drafts it | Platform `pr_drafts` | `pdr_` |
| **Invite draft** | An agent's invite into a session, waiting for the machine's owner | The machine | Its own |
| **Plane write** | An agent's write to a connected Plane project | Platform `tracker_writes` | `tw_` |

Say "the session's PR" for one in `pr_drafts`, so that "draft" next to "PR"
stays GitHub's draft PR, which the session's PR may be sent as. A person
discards a draft; `dropped` stays where it's stored or on the wire.

### Docs

| Kind | What it is | Truth | ID |
|---|---|---|---|
| **Library Doc** | A Doc on a machine | `<state>/docs/<id>/` | 16 hex |
| **Project Doc** | A Doc made on the platform, in a project | Platform `project_docs` | `pdc_` |
| **Shared Doc** | An encrypted snapshot behind a link, frozen until someone updates it | Hub `doc_links`; the key stays out of control | 32 hex |

A Doc has a layout (doc, tabs or prototype; doc when it says none),
kept in its `meta.json`. A Doc's tabs are its sections: in code and on the
wire they stay "sections", because "tab" there is a session's tab.

A Doc has one home, and the context picks it (aru-139): a personal
machine's Docs live on the machine, only there; a team's Docs (made in a team
pane, a session or a team machine) live in the team's project on Arugula's
cloud. Nobody sets it. Joining a team is the one hand-off: "Docs you share
with the team are stored on Arugula's cloud." Moving a personal Doc in, by
onboarding's checklist or Move to team, takes it off the machine and leaves a
hidden pointer, so old links and panes follow it. There are no copies.

Copies went with #865: a machine copies no Doc up, and there is no Share
with… or Update copy. A `meta.json` from before may still say `platform`,
which is ignored. Docs written on the platform in a personal project are
kept in that person's cloud: they can move them to a team, or download them
as Markdown.

## Don't say

Each of these words means something else in Arugula, or meant two things
before. [scripts/data-model-check.mjs](../scripts/data-model-check.mjs) looks
for them in the MCP tools' text
([the blessed tool list](../crates/arugulad-mcp/tests/fixtures/mcp-tool-list.json))
and in the CLI's help, and fails on any outside the phrases this table allows.

| Don't say | For | Say | Allowed in |
|---|---|---|---|
| box, boxes | A Cloud workspace or a hosted machine | Cloud workspace, hosted machine, machine | Hud and studio apps: `hud box`, `studio app box`, `studio app's box`, `an app's box`, `a box with its own agent`, `its box's chat tabs`, `follow the box with the app's follower link`, `the link the box's owner made`. A host reached over ssh: `a box you can ssh into`, `user@box`, `[user@]box`, `https://box.`, `that box, set up over ssh`, `on a box a client reaches over ssh`, `a box over ssh`, `--ssh box` |
| zone, zones | A board's lane or group | lane, group | `time zone`, `--tz ZONE`, `Zone:Read` |

## Don't merge these

Some things look like duplicates but are the security design. Each one is on
purpose, and the records say why:

- A daemon evaluates devices and rosters itself, so control can't add a device
  ([aru-056](decisions/aru-056-trust-is-a-chain-of-device-approvals.md)).
- Revocations only accumulate, so control leaving one out can't readmit a device.
- A hosted certificate is its own domain and approves nothing, and delegation is
  per project rather than a global control key
  ([aru-095](decisions/aru-095-control-enrolls-hosted-machines-under-a-project.md)).
- Machines can't grant themselves powers, and the platform can't forge a standing
  grant ([aru-073](decisions/aru-073-grants-not-impersonation.md)).

## What's not true yet

| Where the code differs | Tracked in |
|---|---|
| What Claude Code says about its conversation is `SessionSignal` in Rust, after the `session-signal` verb and route | Not tracked: CLI verbs stay (#668) |
| `arugula machines` lists only Labs VMs, which have no daemon inside | #670 keeps it, under the `sandbox-providers` flag |
| Nobody can see in one place who can reach a machine, or whose agents hold powers over a project | #681, #682 |
| A write power on a viewer's machine is accepted and lists as live, and a project owner can't revoke a teammate's machine's power over the project | #683 |
| `acl.json`'s row is `Grant` in Rust, and `arugula access grant` is the verb that makes a share | Not tracked: CLI verbs stay (#668) |
| A board's wire says zone: a read's `zones` are the four lanes, and a write's `zone` names a group. Writes take `group` too | Not tracked: wire names stay for older daemons (#750) |
| An issue's comments and the live updates of issues and Docs travel in chat, so a member with chat off reads an issue without its comments and sees a change when the page loads again | Not tracked yet |
| The platform still writes the old project columns (`is_default`, `issue_home`, `agents_model`, `agents_workspace`, `board_settings`), though `scoped_settings` is read instead | [Cleanup](https://app.arugula.io/issues/iss_7cbd2223818266e8e4e3f37cb5fcaa06) drops them in a two-step deploy |
| `Project.team` and the other wire fields the new ones replaced are still sent | [Cleanup](https://app.arugula.io/issues/iss_7cbd2223818266e8e4e3f37cb5fcaa06) drops them once no supported daemon reads them |
| An agent token's powers are given over a project or the DMs, never over a team | Not tracked yet |
