# Managoat

Managoat is a headless agent backend: a conversational API to a computer,
with no console of its own. Arugula is its UI (#942, DECISIONS.md
"Fountain is a headless backend, and Arugula is its UI"): the account's
conversations, their permission requests as cards, a conversation as an
agent block, and agent recipes run in Managoat sandboxes. This page is the
team's description of it; the public docs at
[docs.arugula.io](https://docs.arugula.io/) don't cover it yet.

## Fountain is Managoat

The project was called Fountain until 2026-10-15, when it became Managoat
([its ADR 0082](https://github.com/managoat/managoat/blob/main/decisions/0082-the-project-is-managoat.md),
DECISIONS.md "Fountain is Managoat"). Arugula uses the new names, and
reads each old one as Managoat does, until Managoat stops
([managoat/managoat#2776](https://github.com/managoat/managoat/issues/2776)):

| What | Now | Still read |
|---|---|---|
| The login's variables | `MANAGOAT_API_KEY`, `_BASE_URL`, `_PROFILE` | `FOUNTAIN_*`, when the new one is unset or empty (said once in the daemon's log) |
| The CLI's credentials | `~/.managoat/credentials` | `~/.fountain/credentials`, when there's no new one (said once) |
| This daemon's sign-in and cursor | `managoat/` in the state dir | `fountain/`, moved to `managoat/` at start-up |
| The command | `arugula managoat …`, `arugula agent --managoat`, `arugula agents run --managoat` | `arugula fountain …`, `--fountain` (hidden) |
| The daemon's option | `--managoat-account-stream`, `ARUGULA_MANAGOAT_ACCOUNT_STREAM` | `--fountain-account-stream` (hidden), `ARUGULA_FOUNTAIN_ACCOUNT_STREAM` |
| A saved block | `type: managoat`; `agent: managoat`, `managoat_agent` | `type: fountain`; `agent: fountain`, `fountain_agent`, saved again under the new names |
| MCP | `show` kind `managoat`; `list` kinds `managoat_agents`, `managoat_agent`, `managoat_conversations`; `start_agent {agent: managoat, managoat_agent}`, `{on: managoat}` | the `fountain` names, answered but not listed |
| The flag | none | `fountain` is still a retired flag, and so is `managoat` |

Left as they are, as names nobody but Arugula reads: the HTTP routes
(`/api/fountain/…`, `/api/a2a/fountain/…`) and operation names
(`fountain.login`, …), the block calls (`run_fountain`,
`fountain_preflight`, `fountain_copy`), `features.fountain` in `/api/host`,
the `fountain` key in a diff or file block's config, the agent block's
state, the `ARUGULA_FOUNTAIN_*_MS` test knobs, and the Rust module
`labs/fountain/`. Managoat's own unchanged names (`_fountain/…` ACP
methods, `fountain.sandboxLocations`) stay too.

## Turning it on

There is nothing to turn on. Managoat was a Labs feature behind the
`fountain` flag until #942's F5; now every build with Labs compiled in (the
release builds and the desktop app) offers it, on any machine that has a
Managoat login. A `fountain` the flags file still names, on or off, is
ignored, and `arugulad flags fountain on` (or `managoat`) says so.

- **A login is what it needs.** Sign the daemon in with `arugula managoat
  login URL` (below), or have the `managoat` CLI's login on the machine
  (`$MANAGOAT_API_KEY`, or `~/.managoat/credentials`). Until then Managoat
  is inert: the menus don't offer *Managoat…*, *Run on Managoat* isn't
  shown, and the daemon makes no request to Managoat at all, even with the
  account stream on (`crates/daemon/tests/integration/fountain_account.rs`,
  `no_login_no_requests_to_fountain`).
- **The account stream is still asked for.** `arugulad
  --managoat-account-stream` runs it, on one daemon per account (below).
- **Everywhere it shows.** `arugula managoat` and `arugula agent
  --managoat/--vault` in `arugula --help`; `--managoat-account-stream` in
  `arugulad --help`; MCP's `show` kind `managoat`, `list` kinds
  `managoat_agents`, `managoat_agent` and `managoat_conversations`, and
  `start_agent`'s `managoat_agent`, `on`, `vault` and `environment`;
  `Client.has("fountain")` in the web client, which is the daemon's word
  that it has a login.
- **A build without Labs has none of it.** Managoat's code is still the
  `labs` cargo feature's (`crates/daemon/src/labs/fountain/`; DECISIONS.md
  "Fountain is out of Labs"), so `just check-core`'s build lists none of
  the above, and a Managoat block there says it isn't in this build.

## Sign-in and the account stream

F1 (#942) is the plumbing: the sign-in, the account stream and the client.

- **Sign-in.** `arugula managoat login [URL]` (or `POST
  /api/fountain/login {base_url?}`, for the web and the phone) starts
  Managoat's device flow. It prints a code and where to approve it, in any
  browser that is signed in to Managoat. The daemon polls until the code is
  approved, then keeps the key in `managoat/login.json` in its state dir
  (mode 0600, in a 0700 directory). The key is never logged or sent to a
  client. Without a URL it uses the instance it is signed in to, else
  `$MANAGOAT_BASE_URL` or the `managoat` CLI profile's `base_url`. There is
  no default instance, so with none of those it asks for a URL. `arugula
  managoat status` shows which login is in use. `arugula managoat logout`
  deletes the file and revokes the key on Managoat.
- **Which login.** This daemon's sign-in comes first. Without one, Arugula
  uses the `managoat` CLI's login as before: `$MANAGOAT_API_KEY`, then
  `~/.managoat/credentials`.
- **The account stream.** `arugulad --managoat-account-stream`
  (`ARUGULA_MANAGOAT_ACCOUNT_STREAM=1`) holds one `GET
  /api/events/stream?blocks=true` connection for the login. The option is
  off by default: turn it on for **one daemon per account**, the home
  machine's, so that each permission request raises one card. The stream
  hands every event to in-process subscribers, by conversation or all
  together (`labs/fountain/stream.rs`, `Hub`). The cursor, the last event
  id handled, is kept in `managoat/stream.json`. After a reconnect or a
  restart, including a crash, the stream resumes from the cursor with
  `Last-Event-ID`. An event at or below the cursor is dropped, so none is
  missed and none is handled twice. Managoat closes an idle stream after
  60 s; the stream reconnects at once. Errors back off from 1 s up to a
  minute, or for as long as `Retry-After` asks. When Managoat refuses the
  key, the stream waits for a new sign-in.
- **The client.** `labs/fountain/client.rs` is a typed REST client for what
  the later phases need: conversations (list, show, create, prompts,
  interrupt, wake, terminate, turns, tree, answering requests, labels),
  sandbox files, diffs and git status, agents, apply, environments and
  vaults with their secrets' keys and writes, and the account's inference
  credentials. Errors carry Managoat's codes. A 429 is retried after its
  `Retry-After`.

## The account view and its permission requests

F2 (#942) turns the Managoat block into the account's view. *Managoat…* in
a pane's menu (or the phone's sheet) opens it, on a machine with a login.

- **Conversations** is its first tab: every conversation on the account,
  whoever started it (Arugula, `managoat run`, a script, another
  conversation), most recently active first, 100 at a time (*More* reads
  100 more, up to 500). Each row shows its title (else its agent's name),
  its status, an unread dot, its labels and how many conversations it
  started. A child is listed under its parent, and *+N* shows the whole
  spawn tree (`GET /api/conversations/:id/tree`). Label chips filter with
  Managoat's own `?label=key:value`, combined with AND, and the filter is
  kept in the block. Opening a row opens the conversation in an agent
  block beside the view: an `agent: managoat` block on the native link
  (below), which loads that conversation once and follows it, never
  starting a new one. Opening it marks it read (`POST
  /api/conversations/:id/read`), as the ✓ on an unread row does.
- **Live status.** On the daemon that runs the account stream
  (`--managoat-account-stream`), a turn starting or ending, or a sandbox
  waking, changes its row at once, and a change to the list (Managoat's
  `conversations` signal) reads it again, at most once every 2 s while
  the view is drawn and every 20 s while it isn't
  (`ARUGULA_FOUNTAIN_RELIST_MS` sets the 2 s). It doesn't poll. Managoat
  allows a key 600 requests a minute. Without the stream, the view reads
  the list when it opens, on ↻, and every few minutes while it's drawn.
- **Agents** is the second tab: the catalog as before, read-only, with
  *Run on Managoat* and *Spec*.
- **The login** is the daemon's: its own sign-in, else
  `$MANAGOAT_API_KEY`, else the `managoat` CLI's credentials. A block whose
  config names a CLI profile uses that profile instead.
- **Permission requests.** When an agent in any of the account's
  conversations asks to run a tool, the daemon that runs the account stream
  raises a card and pushes it to the phone at once, because Managoat denies
  an unanswered request after 5 minutes. No block needs to show the
  conversation. The card is on the account view. If none is open, one
  opens in a session called `managoat`. *Allow* (or *Always allow*) and
  *Deny* answer Managoat with one of the options the agent offered (`POST
  /api/conversations/:id/requests/:request_id`). Every open request is
  listed at the top of the view, and the card shows the newest. A phone's
  *Allow* still answers the request it was pushed for, even after a newer
  one came in. When a request is answered anywhere else (the CLI, an
  editor, another app), it times out or its turn ends, its card clears.
  If someone else answered first, Managoat's 409 clears the card and
  Arugula says the request was already settled. A request for a
  conversation that an agent block here already shows is listed but
  raises no card, because that block asks for it. Only the machine's own
  account answers these requests, and only the owner gets their pushes,
  since they're answered with the daemon's own Managoat login. A replay
  after a restart raises only the requests that can still be answered.
- **MCP:** `list` kind `managoat_conversations` (`labels`, `status`,
  `limit`) lets an agent read the account's conversations and their open
  requests. It is read-only: answering stays with the person.

## A conversation as an agent block

F3 (#942). An `agent: managoat` block is a
Managoat conversation, and nothing runs on this machine for it: no
`managoat acp`. The daemon speaks Managoat's API for the block and makes
its transcript from the conversation's events, so its cards, the phone,
`prompt_agent` and `agent_respond` work as for any agent block.

- **Starting one.** `{agent: managoat, managoat_agent, vault?,
  environment?, labels?, permission?, model?, profile?}` creates the
  conversation (agent, vault and environment by name or id; `permission`
  as `managoat acp --permission` takes it, `ask` by default, so approvals
  come to the block). With `session_id` (a conversation that exists) it
  opens that one instead. The login is the daemon's (`arugula managoat
  login`, else the CLI's), or the CLI `profile`'s.
- **Prompts** go with a `client_request_id` (`arugula-BLOCK-N`), which finds
  the turn each opened; that turn's end answers it, with the sandbox's own
  stop reason. What ACP has no word for is said as an error: a sandbox that
  never started or was reclaimed mid-turn, a terminated conversation, a
  refused admission, a failed wake. A wake that failed retryably sends the
  prompt again by itself. Another client's turn, or one Managoat started,
  shows in the transcript as it runs.
- **Restarts.** Each frame made from an event names it, so the block's log
  is its cursor: a restarted daemon follows on from the last event it took,
  with nothing replayed and nothing twice, and a prompt sent before the
  restart is answered when its turn ends. A block saved by a daemon that ran
  `managoat acp` reloads its conversation once (`session/load`, merged),
  then has a cursor. Closing a block stops nothing on Managoat.
- **Events** come from the account stream's fan-out on the daemon that runs
  it (`--managoat-account-stream`), else from the conversation's own stream.
- **Header.** The status line (pending, running, idle·awake, idle·parked,
  failed, terminated), *Wake*, *Interrupt*, *Terminate* (it asks first: the
  sandbox's disk goes; the owner's only), and *Changes* and *Files*.
- **Sandbox panes.** *Changes* is a diff block and *Files* a file block
  (`{fountain: {conversation, profile?}}` in their config) that read the
  sandbox through Managoat (`GET /api/sandboxes/:id/{diff,git-status,files,
  file}`): the tracked diff with what's untracked listed beside it, a
  directory's entries, a file. A parked sandbox isn't woken by a read: they
  say "Parked — Wake to look", with *Wake*.
- `fountain conv show`'s polling (`REMOTE_POLL`) is gone. To run `managoat
  acp` by hand, use an `agent: acp` block with that command.

## Recipes on Managoat

F4 (#942), with the `agents` flag on, since recipes are still Labs (see
[labs.md](labs.md#agent-recipes)). An agent recipe
runs in a Managoat sandbox as well as here or on another of your machines:
*Run on Managoat* sits beside *Run on THIS-MACHINE* on its card in the agents
block, and on the Agents page. MCP's `start_agent {recipe, on: managoat,
vault?, environment?}`, the CLI's `arugula agents run NAME --managoat
[--vault V] [--environment E] [--cwd DIR]` and the block's `run_fountain
{agent, cwd?, prompt?, vault?, environment?}` do the same.

- **One Managoat agent per recipe** (Jake, 2026-10-15). Before each launch
  the recipe is compiled and upserted with `POST /api/apply` as
  `recipe-NAME`, with `metadata: {managed-by: arugula, source_path,
  recipe_hash}`. An unchanged recipe applies as `unchanged` and writes
  nothing. Editing the recipe changes that agent, so an older conversation
  gets the new system prompt on its next wake. Each conversation is
  labelled `arugula.recipe`, `arugula.recipe_hash` and `arugula.machine`,
  so the hash says which version started it. An agent of that name that
  Arugula didn't make (chant's, or one made by hand) is never written, and
  the run is refused.
- **What carries over.** The body becomes `system`. `sonnet`, `opus` and
  `haiku` become `anthropic/claude-sonnet-5-5`, `anthropic/claude-opus-5-5`
  and `anthropic/claude-haiku-5-5`, and a `claude-…` id gets the prefix.
  `inherit` leaves the agent's model alone, and a new agent starts on
  sonnet. `mcpServers` keep their `${VAR}`s as written, and Managoat fills
  them from the environment's and the vault's secrets at spawn.
- **What doesn't, and is named on the run** (the dialog lists it, as M76's
  header does):
  - a skill's files other than `SKILL.md`, because Managoat's inline skills
    carry `SKILL.md` only;
  - a server with a value typed into its headers or env rather than a
    `${VAR}`, so that no credential leaves this machine without the copy
    below;
  - a server that wants an OAuth sign-in, or whose command is a path on
    this machine;
  - a model Managoat can't name;
  - the tool lists and permission mode. These become a coarse
    `permission_policy` over ACP's kinds. With no `Bash`, `execute` is
    denied. With no `Edit` or `Write`, `edit`, `delete` and `move` are
    denied. With no `WebFetch` or `WebSearch`, `fetch` is denied. A tool's
    rule (`Bash(git:*)`) is lost. `plan` denies edits and commands, and
    `acceptEdits` allows edits. Everything else asks, so approvals come to
    the block. The launch sends the same policy, so it never widens the
    agent's.
- **The working directory.** The sandbox clones its own checkout. The
  project's `origin`, as https and with no credentials, becomes an
  environment `repo-OWNER-NAME` with `repositories: [{url, mount_path:
  /work/NAME}]`, and `secret_key: GITHUB_TOKEN` on GitHub. It goes in the
  same apply. Managoat clones the default branch, so uncommitted changes,
  unpushed commits and a branch that isn't on `origin` stay here. The
  dialog says so. An environment of that name that Arugula didn't make is
  used as it is. Choosing another environment replaces the project's.
- **Secrets: a preflight and an explicit copy.** The dialog
  (`fountain_preflight`, `POST /api/a2a/fountain/preflight`) compares the
  `${VAR}`s the agent uses (and `GITHUB_TOKEN` for a GitHub clone) with
  the keys of the run's environment and vault. Managoat lists keys, never
  values. It shows "N variables missing on Managoat", and each missing
  variable has *Copy from this machine to vault V*
  (`fountain_copy`, `POST /api/a2a/fountain/copy`). The copy resolves the
  variable here as wearing does (Infisical through `~/agent-specs`, your
  shell, `gh auth token`) and writes it to the vault once. Nothing is
  copied automatically, a variable the recipe doesn't use can't be copied,
  and no value is shown, logged or kept.
- Running, previewing and copying are this machine's own account's.

## Default tool approvals

The owner can choose **Permission rules… → New Managoat conversations**:
*Ask for approval* (the default) or *Automatically approve*. This preference
is saved on the current machine, applies to new UI, CLI and MCP agent-block
launches, and leaves local agents and existing conversations unchanged.
It is not yet an account preference synced between machines.

`arugula rules --managoat-approvals auto_allow` saves the same preference;
`--managoat-approvals ask` resets it. For one launch, use
`arugula agent --managoat NAME --permission ask` (or `auto_allow`), or the
start-agent dialog's **Tool approvals** choice. An explicit block-config
`permission` wins over the default, including policies supplied by recipes.

Managoat can still reject a policy wider than the agent allows. Automatic
tool approval does not change the runtime's own sandbox or grant vault
access. Codex's full-access environment setting is a separate choice.
