# Arugula control

Using control (signing in, devices, joining machines, teams) is on
**[docs.arugula.io/control](https://docs.arugula.io/control/)**. Operating the
hosted one, running your own (unsupported for now, #662) and testing it are in
[control-ops.md](control-ops.md); the design in [control-e2e.md](control-e2e.md).

**A joined machine acts as its owner in their Cloud workspaces (#1094).**
Control takes a daemon's signature on the Cloud workspace and tab routes
(list, create, show, the machine's files, diff, git status and setup log,
tabs, prompts, interrupts, answers, rebuild and retry, and the agent and
model lists) and scopes it to the daemon's account, as it would that
account's own session. The agents on that machine drive those Cloud
workspaces through MCP ([managoat.md](managoat.md#cloud-workspaces)), and
can put one beside a pane as a Cloud workspace block (#1117), whose reads
and writes go the same way.
**A teammate with run rights on a machine can therefore drive its owner's
Cloud workspaces through the agents there.** That was decided, not
overlooked. A daemon can't change a Cloud workspace's settings, delete one,
or reach secrets, the agent library or past tabs; those stay a person's
(`cloud::CloudDriver` in `crates/platform/src/cloud/mod.rs`).

**That power is a grant (M130, #1126).** Every machine joined to an account
holds a `cloud.drive` grant over it: machines joined before M130 got one
when control started, and each machine that joins gets one as soon as it's
approved (from control's change feed, or the first time it drives, whichever
comes first, so a machine that just joined is never refused). The routes
above check it, so a machine without a live grant is
refused (403) with a message that names it and says how to give it back.
The owner manages them from a device logged in to the account:

```
arugula grants                  # every machine's grants, live and revoked
arugula grants revoke geek      # geek's agents can't drive your Cloud workspaces
arugula grants restore geek     # they can again (a new grant; the old one stays revoked)
```

or through control's API, as a signed-in person or a CLI device (a
machine's signature isn't accepted here): `GET /api/grants`,
`POST /api/grants/{id}/revoke`, and `POST /api/grants
{"daemon": "<machine id>", "power": "cloud.drive"}`. Grants are per machine,
since the daemon signs every MCP call as itself; narrowing one to the
owner's own agents is a later daemon change. A machine that's removed loses
its grants, and one that joins again gets a new one. Grants live in
the platform's `grants` table (`crates/platform/src/db/projects.rs`, "Grants,
not impersonation" in [DECISIONS.md](../DECISIONS.md)), and the routes are
the platform's behind the hub's gate (`crates/platform/src/grants.rs`).

**Linked repos (M163, #1209).** A GitHub repo is linked to one project, by
a member of it (the account, for a personal project; a team's owner or
editor) who is an admin or maintainer of the repo on GitHub. Control checks
that by the person's numeric GitHub id (from signing in with GitHub),
through a read-only installation token of its GitHub App. Where the App
isn't installed for the repo, the answer is a 409 with `reason:
"install_app"` and GitHub's page to install it, whose `state` is
`<project>:<owner>/<name>`; the App's setup URL is the platform page's
`/installed`, which brings the person back to the project's settings with
the repo filled in. The same machines that drive Cloud workspaces link for
their owner (a live `cloud.drive` grant), so agents can through MCP
(`list` kind `linked_repos`, `link_repo`).

```
arugula project ls                        # your personal project's linked repos
arugula project link acme/app             # --project <id or name> for another
arugula project unlink acme/app
```

Unlinking stops reading GitHub for the repo at once. So does the App's
installation being deleted or suspended, or the repo being taken out of it
(its `installation` and `installation_repositories` webhooks). The repo's
rows (items, checks, events, content, drafts and inbox rows) go within a day
(`purge_after`, `crates/platform/src/forge/links.rs`) unless it's linked
again to the same project first. Linking it to another project instead
purges the old project's rows at once.

**Bodies and comments (M166, #1212).** The platform keeps a linked repo's
PRs and issues as metadata. Opening one (`GET
/api/forge/items/{id}/content`) reads its body, comments, reviews' text
and review comments from GitHub, after the same viewer check as every
read, with a read-only token minted for the reader. They're kept about 3
minutes in memory on that machine, never in the database, and a webhook
for the item drops them. A project can mirror a repo's content instead:
on its settings page, or `PATCH /api/projects/{project}/repos/{owner}/{name}`
with `{"mirror_content": true}`. Turning it on takes what linking takes (a
member who maintains the repo on GitHub). Then webhooks and the poller
keep the bodies and comments in `forge_content`, reads come from there,
and the project's search (`q`) finds their text. Turning it off (any member
who may change links) deletes them at once. Unlinking, or the App being
uninstalled, purges them within a day.

**Forge operations (M169, #1215).** A linked repo's PRs and issues are read
and drafted on without a block, or a machine awake: one route, `POST
/api/forge/ops`, takes a typed operation by name and version (`{"op":
"pr.get", "v": 1, "repo": "acme/app", "number": 7}`) and answers `{"op",
"v", "result"}` (`arugula_control_wire::forge_ops`). Each operation has its
own version; one this control doesn't speak is a 400 that says which it
does. Reads (`pr.get`, `issue.get`, `item.list`, `checks.get`) and drafts
(`draft.create`, `draft.list`, `draft.get`, `draft.discard`) go through the
same viewer check as the routes above (M165), for a person or a machine with
a live `cloud.drive` grant (for its owner). Sends (`draft.send`,
`pr.comment`, `pr.review`, `issue.create`, `issue.comment`) are a person's
only: a machine is refused (403, `reason: "agents_draft"`). A person's goes
out as them through one of their machines (M168, below): a terminal has no
browser for GitHub's authorization. A comment, review or issue that isn't a
draft yet is made one first, so one that can't go now waits as a draft, and
the refusal names it.
The inbox (M171): `inbox.list` reads the reader's (a person's, or a
machine's for its owner), and `inbox.mark` marks an item's entries seen or
done, a person's only (a machine is refused, 403).

**The inbox (M171, #1217).** The platform works out each member's inbox
from the mirror's rows, by a forge block's attention rules: a review
requested from you, your PR's checks failed, changes requested on it, a
mention (only where the project mirrors content), an issue assigned to
you, your PR merged or approved and green (your issue closed), and a draft
waiting for you. A team's review request isn't one: GitHub tells the
platform nobody's teams. Only members the viewer check lets read the item
get an entry, or a push. A reason newly holding pushes once, through the
hub. The platform page shows it at `/inbox`, grouped by reason, with Seen
and Done; `GET /api/inbox` and `POST /api/inbox/{item}` are its routes.

Agents use them through MCP: `forge_read` (kinds `pr`, `issue`, `items`,
`checks`, `inbox`, `drafts`, `draft`) and `forge_draft` (kinds `comment`, `review`,
`issue`, `discard`). MCP has no send. `read_forge` on a block of a linked
repo reads the platform's item too (`platform` in its answer), and `draft`
on one keeps the draft on the platform (M170).

**Pins (M172, #1218).** A PR or issue block on a repo linked for the
machine's owner is a pin onto the platform's item (`pin: {kind,
global_id}` in its config and state, kept in `layout.json`): the daemon
reads the item, its checks and its events from the platform, at most every
10 seconds while it's drawn and at once on a poke, and makes no GitHub call
for it. It looks as a block read from GitHub does, less the description
and comments' text, which the mirror doesn't keep. Unlinked repos, Forgejo,
GitLab and machines outside control read as before. The platform says it's
unlinked: the block reads GitHub again. The platform can't be reached: the
block shows what it said last for 10 minutes, then reads GitHub until it
answers (after a restart, at once: a block is never blank). A person's
comment, review or merge from a pinned block still goes out with their `gh`
until sends are the platform's (M168). A machine reporting for M171's
shadow period (`ARUGULA_INBOX_SHADOW`) doesn't pin, so its report stays
GitHub's word.

```
arugula pr view acme/app#7                # no block; also `issue view`
arugula pr list acme/app --state open     # or --project; also `issue list`
arugula pr checks acme/app#7
arugula pr review acme/app#7 approve      # sends as you (M168); --draft keeps a draft
arugula pr comment acme/app#7 "lgtm"      # likewise `issue comment acme/app#2 ...`
arugula issue create acme/app -t "Title" -b "Body"
arugula draft ls                          # your waiting drafts; or OWNER/REPO[#N], --project
arugula draft show dft_…                  # discard, send
arugula forge inbox                       # what wants you; --all with what's done
arugula forge done acme/app#7             # seen likewise; --reason, --undo
```

Run by an agent (`CLAUDECODE` or `AI_AGENT` set), the CLI's comment,
review and new issue are drafts, and `draft send` is refused. `%N` instead
of `OWNER/REPO#N` still writes to that block.

**Sending as the person (M168, #1214; aru-085).** A draft on a linked repo
goes out on GitHub as its person, never as `arugula[bot]`, so a review
counts as theirs. Only its own person sends it (a person, or their CLI
device); a machine is refused whatever grants it holds, so an agent's draft
waits for its person ("Agents draft, people send", aru-041). A send holds
the draft for up to two minutes (`sending` on the wire), so the phone and a
block's card at once never post it twice. Two ways, then a clear refusal:
1. **A one-use user token.** The platform page's *Send* asks `POST
   /api/forge/drafts/{id}/send` with `via: "browser"`, and opens the page
   it answers: GitHub's authorization of the App, run by the hub (it owns
   the App's OAuth client). It's silent once the person has authorized the
   App, which signing in with GitHub did. GitHub comes back to the hub's
   own `/auth/github/callback` with a state `send.<nonce>` (random, kept in
   the hub's memory ten minutes, good once, for that account's session on
   control's site). The hub swaps the code for a user token and hands it to
   the platform over the signed internal API (`PlatformPort::send_draft`).
   The platform logs the mint (`github_token_mints`, kind `user`, never the
   token), checks `GET /user` is the account's own GitHub id, and makes the
   one call: the comment, review or issue. The hub then revokes the token
   (`DELETE /applications/{client_id}/token`) and sends the browser back to
   the platform page with `?draft_send=<id>`, which shows what became of it.
   The token is never stored, logged, cached or put in a URL. No App
   setting changes: it's the callback sign-in already uses.
2. **Their machine.** With no browser hop (the CLI, the hub without the
   App's client, GitHub declining, or a token that may not write there),
   the platform asks the person's own machines on control's relay, the one
   that drafted it first, to send it with their own `gh` login: the typed
   op `forge.send` (`arugula_control_wire::forge::DaemonOp`), relayed by
   the hub (`HubPort::daemon_op`). A daemon says which ops it knows when it
   connects; one too old to say is skipped, as is one without a `gh` login,
   for the next. A daemon that doesn't answer within 20 s may have sent it,
   so the draft is `failed` (to check and send again), not tried elsewhere.
3. **Otherwise** it's refused (409, `reason: "no_way"`): "Open this on a
   device to send, or wake a machine." The draft waits, saying so. GitHub
   refusing it leaves it `failed` (502, `reason: "github_refused"`), to send
   again.

A block's card on a linked repo sends a platform draft from its machine
with the owner's login, as its own drafts go, and tells the platform
(`POST /api/forge/drafts/{id}/machine`: `start`, then `sent` or `failed`).

**Agents in conversations (M197).** Three more powers let a machine's
agents into its owner's conversations: `threads.read` and `threads.post`
over a project (a team's channels are its default project's; an object's
thread is its project's) or over the owner's DMs, and `issues.write` over a
project. They're off until the owner gives them, and only over what the
owner reads; a machine's agents never reach more than its owner, so one
whose owner leaves the team is refused there at once (within the platform's
cache delay), grant or not.

```
arugula grants add geek threads.read threads.post --project acme
arugula grants add geek threads.read --dms
arugula grants revoke geek threads.post --project acme
```

or a switch per machine and project (and one for your DMs) under Settings,
Agents in conversations, on the platform page. Through the API it's `POST
/api/grants {"daemon", "power", "scope": {"kind": "project", "id"}}` (or
`{"kind": "account"}` for your DMs). Every call an agent makes asks for the
grant again (`grants::agent_may`), so a revoked one refuses the next call,
with a message naming the machine and how to grant it. What an agent posts
is its owner's, labelled "Jake's agent on geek" wherever it shows, and stays
labelled after a revoke. Every room an agent grant reaches says so to
everyone in it: "Jake's agents on geek can read and post here".

Grants bound what a machine's own signature can do (its MCP calls), not
what a shell on it can do with your CLI login. The CLI's device key lives
on the same machine as its agents, so an agent with a shell there can act
through `arugula` as you, unlabelled, `arugula grants add` included (as it
can `arugula grants restore` for `cloud.drive` today).

**Agents reading and posting (M198).** With those grants, the `arugula`
MCP server's agents on that machine use four tools, each a call to the
platform signed by the machine (the daemon keeps no thread of its own):
`list` kind `threads` (the rooms its grants reach, with the newest message
that mentions it), `read_thread` (a room, or an issue's or PR's thread by
its id, paged by `seq`; `mentions_only` for what mentions it),
`search_threads`, and `post_thread`. The reads are in the permission
snippet's allow list and the post in its ask list; an agent a guest stands
behind is refused. `@` and a machine's name (`@geek`, or `@geek's agent`)
in a room its `threads.read` grant reaches is a mention of its agents
(posting or `issues.write` alone isn't: they couldn't read it), kept as
`machine:<id>` among the message's mentions. It pushes nobody, and nothing
wakes an agent: they read their mentions when they look. `capabilities`
says whether the thread tools are live and what they reach. The routes are
`/api/daemon/chat/rooms`, `/api/daemon/chat/rooms/{id}/messages` (`GET`,
`POST`) and `/api/daemon/chat/search`
(`crates/platform/src/chat/agent_threads.rs`).

**Agents on native issues (M200).** The same machine's agents read and
write a project's native issues over MCP. An issue is `#N` in the current
project (the call's `project`, else the one the agent's clone's GitHub
repo is linked to, else the owner's personal project; a clone of a repo
none of the owner's projects links is refused, not guessed) or its
`iss_…` id. Reads: `read_forge` with `block` `#N` or `iss_…`, and
`forge_read` kind `issue` (the same) and `native_issues` (a list or a
search). Writes: `issue_write`, kinds `create`, `comment`, `edit`,
`label`, `assign`, `close` and `reopen`. They land at once, as the
owner's, labelled with the machine: the owner's role is the ceiling, and
the machine needs `issues.write` to file and change (a change reads first,
with `threads.read`) and `threads.post` to comment. A GitHub issue or PR
still gets a draft a person sends (`draft`, `forge_draft`; aru-041,
aru-085); given a native issue they refuse and point to `issue_write`.
`issue_write` is in the permission snippet's ask list. The routes are
`/api/daemon/issue-home`, `/api/daemon/projects/{project}/issues[/{number}]`
and `/api/daemon/issues/{id}[/events]`
(`crates/platform/src/issues/agent.rs`); a comment is a post to the
issue's thread by its id.
