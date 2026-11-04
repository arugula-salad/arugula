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
| **Role** | Owner, editor or viewer | The roster, or a session's grant in `acl.json` | The word |
| **Guest** | Someone with access who isn't a member | The machine | Depends on the kind |

**Grant** has several meanings. Say which one you mean:

| Kind | What it says | Truth |
|---|---|---|
| Session grant | A person's role on a session | `<state>/acl.json` |
| Project grant | What a machine's agents may do in a project, like `boards.write` | Platform, `grt_` |
| Standing run grant | Runs and agent sessions a project's owner has signed for | `run_grants`, signed with a passkey |

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
for either, because it also means hud and studio app boxes.

Hosted machines stay out of the `daemons` table on purpose, so that what acts on
ordinary machines (moves, shares, grants) can't reach them.

Other things called a workspace aren't this: a **chant workspace** is a Labs block
over a repo directory, and the **shell's workspace** is its layout.

## Work on a machine

| Word | What it means | Truth | ID |
|---|---|---|---|
| **Session** | A group of tabs on a machine, and the unit of sharing | `layout.json` | `$N` |
| **Tab** | A tab in a session | `layout.json` | `@N` |
| **Pane** | A process on a PTY, or any leaf of a tab | `layout.json`, `blocks/<id>/` | `%N` |
| **Block** | A typed leaf: agent, browser, Doc, editor and so on | `layout.json`, `blocks/<id>/` | `%N`, the same range as panes |
| **Agent** | A live agent in a pane or agent block, addressed as `machine/name` | Worked out live | Its title's slug |
| **Agent session** | Work the platform dispatches to a hosted machine | Platform `agent_sessions`; the machine keeps a ledger of it | `ses_` |
| **Conversation** | A Claude Code conversation on a machine | Claude Code's files in `~/.claude/` | Claude Code's |
| **Run** | A project's run on a hosted machine. Not the same as running a command in a pane | Platform `runs`; the machine keeps a ledger | `run_` |
| **Deploy** | A plan, its approval and its apply | Platform `deploys` | `dep_` |

So "session" alone means the group of tabs. `ses_` is always "agent session", and
Claude Code's own session is a conversation.

## Chat

Chat's truth is on the platform. The daemon keeps only routing state, such as who
may read each pane's thread.

| Word | What it is | ID |
|---|---|---|
| **Room** | Any place people chat: every `chat_rooms` row | Depends on the kind |
| **Channel** | A team's room | `ch_` |
| **DM** | A room for up to 8 people | `dm_` |
| **Object thread** | A room on an issue, PR, workspace, run, agent session, board or Doc | `ob_` |
| **Pane thread** | A room about a session or pane on a machine | `th_` |
| **Mention** | Naming a person, machine or agent in a message | Room + seq + machine |

## What a project holds

A **project** (`prj_`) is what platform objects belong to, owned by one team or one
account. Each has one default project, and that's all that exists for now: nothing
creates a second one. Membership comes from the team's roster.

| Noun | What it is | Truth | ID |
|---|---|---|---|
| **Linked repo** | A GitHub repo tied to one project | Platform `linked_repos` | GitHub's |
| **Native issue** | A project's own numbered issue | Platform `issues` | `iss_` + number |
| **Forge item** | A mirrored PR, GitHub issue or Plane item | GitHub or Plane; `forge_items` is a mirror | `fi_` |
| **Board** | Rows of work with a goal | Platform `boards` | `brd_` |
| **Row** | One piece of work on a board | Platform `board_entries` | `row_` |
| **Group** | A named section of a board | Platform `board_entries` | `zon_` |
| **Lane** | Next, Doing, Review or Done, worked out from the work, with a person's override | Derived on read | The word |

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
`issues` row, read with the machine's `threads.read` grant, never from a
forge.

### Docs

| Kind | What it is | Truth | ID |
|---|---|---|---|
| **Library Doc** | A Doc on a machine | `<state>/docs/<id>/` | 16 hex |
| **Project Doc** | A Doc made on the platform, in a project | Platform `project_docs` | `pdc_` |
| **Copy** | A library Doc's copy in its owner's personal project. Read-only: the machine's Doc is the truth | Machine | `pdc_` |
| **Shared Doc** | An encrypted snapshot behind a link, frozen until someone updates it | Hub `doc_links`; the key stays out of control | 32 hex |

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
| "Session" and "agent session" name other things in Rust types | #667, #671 |
| A Doc's copy can be edited on the platform, and the machine's next change overwrites the edit | #678, #673 |
| MCP and the UI say "thread" and "conversation" for any room | #667 |
| `arugula machines` lists only Labs VMs, which have no daemon inside | #669 |
| How session grants and project grants relate isn't settled | #676 |
