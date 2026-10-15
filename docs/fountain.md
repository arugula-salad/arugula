# Fountain

Fountain is a headless agent backend: a conversational API to a computer,
with no console of its own. Arugula is its UI (#942, DECISIONS.md
"Fountain is a headless backend, and Arugula is its UI"): the account's
conversations, their permission requests as cards, a conversation as an
agent block, and agent recipes run in Fountain sandboxes. This page is the
team's description of it; the public docs at
[docs.arugula.io](https://docs.arugula.io/) don't cover it yet.

## Turning it on

There is nothing to turn on. Fountain was a Labs feature behind the
`fountain` flag until #942's F5; now every build with Labs compiled in (the
release builds and the desktop app) offers it, on any machine that has a
Fountain login. A `fountain` the flags file still names, on or off, is
ignored, and `arugulad flags fountain on` says so.

- **A login is what it needs.** Sign the daemon in with `arugula fountain
  login URL` (below), or have the `fountain` CLI's login on the machine
  (`$FOUNTAIN_API_KEY`, or `~/.fountain/credentials`). Until then Fountain
  is inert: the menus don't offer *Fountain…*, *Run on Fountain* isn't
  shown, and the daemon makes no request to Fountain at all, even with the
  account stream on (`crates/daemon/tests/integration/fountain_account.rs`,
  `no_login_no_requests_to_fountain`).
- **The account stream is still asked for.** `arugulad
  --fountain-account-stream` runs it, on one daemon per account (below).
- **Everywhere it shows.** `arugula fountain` and `arugula agent
  --fountain/--vault` in `arugula --help`; `--fountain-account-stream` in
  `arugulad --help`; MCP's `show` kind `fountain`, `list` kinds
  `fountain_agents`, `fountain_agent` and `fountain_conversations`, and
  `start_agent`'s `fountain_agent`, `on`, `vault` and `environment`;
  `Client.has("fountain")` in the web client, which is the daemon's word
  that it has a login.
- **A build without Labs has none of it.** Fountain's code is still the
  `labs` cargo feature's (`crates/daemon/src/labs/fountain/`; DECISIONS.md
  "Fountain is out of Labs"), so `just check-core`'s build lists none of
  the above, and a Fountain block there says it isn't in this build.

## Sign-in and the account stream

F1 (#942) is the plumbing: the sign-in, the account stream and the client.

- **Sign-in.** `arugula fountain login [URL]` (or `POST
  /api/fountain/login {base_url?}`, for the web and the phone) starts
  Fountain's device flow. It prints a code and where to approve it, in any
  browser that is signed in to Fountain. The daemon polls until the code is
  approved, then keeps the key in `fountain/login.json` in its state dir
  (mode 0600, in a 0700 directory). The key is never logged or sent to a
  client. Without a URL it uses the instance it is signed in to, else
  `$FOUNTAIN_BASE_URL` or the `fountain` CLI profile's `base_url`. There is
  no default instance, so with none of those it asks for a URL. `arugula
  fountain status` shows which login is in use. `arugula fountain logout`
  deletes the file and revokes the key on Fountain.
- **Which login.** This daemon's sign-in comes first. Without one, Arugula
  uses the `fountain` CLI's login as before: `$FOUNTAIN_API_KEY`, then
  `~/.fountain/credentials`.
- **The account stream.** `arugulad --fountain-account-stream`
  (`ARUGULA_FOUNTAIN_ACCOUNT_STREAM=1`) holds one `GET
  /api/events/stream?blocks=true` connection for the login. The option is
  off by default: turn it on for **one daemon per account**, the home
  machine's, so that each permission request raises one card. The stream
  hands every event to in-process subscribers, by conversation or all
  together (`labs/fountain/stream.rs`, `Hub`). The cursor, the last event
  id handled, is kept in `fountain/stream.json`. After a reconnect or a
  restart, including a crash, the stream resumes from the cursor with
  `Last-Event-ID`. An event at or below the cursor is dropped, so none is
  missed and none is handled twice. Fountain closes an idle stream after
  60 s; the stream reconnects at once. Errors back off from 1 s up to a
  minute, or for as long as `Retry-After` asks. When Fountain refuses the
  key, the stream waits for a new sign-in.
- **The client.** `labs/fountain/client.rs` is a typed REST client for what
  the later phases need: conversations (list, show, create, prompts,
  interrupt, wake, terminate, turns, tree, answering requests, labels),
  sandbox files, diffs and git status, agents, apply, environments and
  vaults with their secrets' keys and writes, and the account's inference
  credentials. Errors carry Fountain's codes. A 429 is retried after its
  `Retry-After`.

## The account view and its permission requests

F2 (#942) turns the Fountain block into the account's view. *Fountain…* in
a pane's menu (or the phone's sheet) opens it, on a machine with a login.

- **Conversations** is its first tab: every conversation on the account,
  whoever started it (Arugula, `fountain run`, a script, another
  conversation), most recently active first, 100 at a time (*More* reads
  100 more, up to 500). Each row shows its title (else its agent's name),
  its status, an unread dot, its labels and how many conversations it
  started. A child is listed under its parent, and *+N* shows the whole
  spawn tree (`GET /api/conversations/:id/tree`). Label chips filter with
  Fountain's own `?label=key:value`, combined with AND, and the filter is
  kept in the block. Opening a row opens the conversation in an agent
  block beside the view: an `agent: fountain` block on the native link
  (below), which loads that conversation once and follows it, never
  starting a new one. Opening it marks it read (`POST
  /api/conversations/:id/read`), as the ✓ on an unread row does.
- **Live status.** On the daemon that runs the account stream
  (`--fountain-account-stream`), a turn starting or ending, or a sandbox
  waking, changes its row at once, and a change to the list (Fountain's
  `conversations` signal) reads it again, at most once every 2 s while
  the view is drawn and every 20 s while it isn't
  (`ARUGULA_FOUNTAIN_RELIST_MS` sets the 2 s). It doesn't poll. Fountain
  allows a key 600 requests a minute. Without the stream, the view reads
  the list when it opens, on ↻, and every few minutes while it's drawn.
- **Agents** is the second tab: the catalog as before, read-only, with
  *Run on Fountain* and *Spec*.
- **The login** is the daemon's: its own sign-in, else
  `$FOUNTAIN_API_KEY`, else the `fountain` CLI's credentials. A block whose
  config names a CLI profile uses that profile instead.
- **Permission requests.** When an agent in any of the account's
  conversations asks to run a tool, the daemon that runs the account stream
  raises a card and pushes it to the phone at once, because Fountain denies
  an unanswered request after 5 minutes. No block needs to show the
  conversation. The card is on the account view. If none is open, one
  opens in a session called `fountain`. *Allow* (or *Always allow*) and
  *Deny* answer Fountain with one of the options the agent offered (`POST
  /api/conversations/:id/requests/:request_id`). Every open request is
  listed at the top of the view, and the card shows the newest. A phone's
  *Allow* still answers the request it was pushed for, even after a newer
  one came in. When a request is answered anywhere else (the CLI, an
  editor, another app), it times out or its turn ends, its card clears.
  If someone else answered first, Fountain's 409 clears the card and
  Arugula says the request was already settled. A request for a
  conversation that an agent block here already shows is listed but
  raises no card, because that block asks for it. Only the machine's own
  account answers these requests, and only the owner gets their pushes,
  since they're answered with the daemon's own Fountain login. A replay
  after a restart raises only the requests that can still be answered.
- **MCP:** `list` kind `fountain_conversations` (`labels`, `status`,
  `limit`) lets an agent read the account's conversations and their open
  requests. It is read-only: answering stays with the person.

## A conversation as an agent block

F3 (#942). An `agent: fountain` block is a
Fountain conversation, and nothing runs on this machine for it: no
`fountain acp`. The daemon speaks Fountain's API for the block and makes
its transcript from the conversation's events, so its cards, the phone,
`prompt_agent` and `agent_respond` work as for any agent block.

- **Starting one.** `{agent: fountain, fountain_agent, vault?,
  environment?, labels?, permission?, model?, profile?}` creates the
  conversation (agent, vault and environment by name or id; `permission`
  as `fountain acp --permission` takes it, `ask` by default, so approvals
  come to the block). With `session_id` (a conversation that exists) it
  opens that one instead. The login is the daemon's (`arugula fountain
  login`, else the CLI's), or the CLI `profile`'s.
- **Prompts** go with a `client_request_id` (`arugula-BLOCK-N`), which finds
  the turn each opened; that turn's end answers it, with the sandbox's own
  stop reason. What ACP has no word for is said as an error: a sandbox that
  never started or was reclaimed mid-turn, a terminated conversation, a
  refused admission, a failed wake. A wake that failed retryably sends the
  prompt again by itself. Another client's turn, or one Fountain started,
  shows in the transcript as it runs.
- **Restarts.** Each frame made from an event names it, so the block's log
  is its cursor: a restarted daemon follows on from the last event it took,
  with nothing replayed and nothing twice, and a prompt sent before the
  restart is answered when its turn ends. A block saved by a daemon that ran
  `fountain acp` reloads its conversation once (`session/load`, merged),
  then has a cursor. Closing a block stops nothing on Fountain.
- **Events** come from the account stream's fan-out on the daemon that runs
  it (`--fountain-account-stream`), else from the conversation's own stream.
- **Header.** The status line (pending, running, idle·awake, idle·parked,
  failed, terminated), *Wake*, *Interrupt*, *Terminate* (it asks first: the
  sandbox's disk goes; the owner's only), and *Changes* and *Files*.
- **Sandbox panes.** *Changes* is a diff block and *Files* a file block
  (`{fountain: {conversation, profile?}}` in their config) that read the
  sandbox through Fountain (`GET /api/sandboxes/:id/{diff,git-status,files,
  file}`): the tracked diff with what's untracked listed beside it, a
  directory's entries, a file. A parked sandbox isn't woken by a read: they
  say "Parked — Wake to look", with *Wake*.
- `fountain conv show`'s polling (`REMOTE_POLL`) is gone. To run `fountain
  acp` by hand, use an `agent: acp` block with that command.

## Recipes on Fountain

F4 (#942), with the `agents` flag on, since recipes are still Labs (see
[labs.md](labs.md#agent-recipes)). An agent recipe
runs in a Fountain sandbox as well as here or on another of your machines:
*Run on Fountain* sits beside *Run on THIS-MACHINE* on its card in the agents
block, and on the Agents page. MCP's `start_agent {recipe, on: fountain,
vault?, environment?}`, the CLI's `arugula agents run NAME --fountain
[--vault V] [--environment E] [--cwd DIR]` and the block's `run_fountain
{agent, cwd?, prompt?, vault?, environment?}` do the same.

- **One Fountain agent per recipe** (Jake, 2026-10-15). Before each launch
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
  sonnet. `mcpServers` keep their `${VAR}`s as written, and Fountain fills
  them from the environment's and the vault's secrets at spawn.
- **What doesn't, and is named on the run** (the dialog lists it, as M76's
  header does):
  - a skill's files other than `SKILL.md`, because Fountain's inline skills
    carry `SKILL.md` only;
  - a server with a value typed into its headers or env rather than a
    `${VAR}`, so that no credential leaves this machine without the copy
    below;
  - a server that wants an OAuth sign-in, or whose command is a path on
    this machine;
  - a model Fountain can't name;
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
  same apply. Fountain clones the default branch, so uncommitted changes,
  unpushed commits and a branch that isn't on `origin` stay here. The
  dialog says so. An environment of that name that Arugula didn't make is
  used as it is. Choosing another environment replaces the project's.
- **Secrets: a preflight and an explicit copy.** The dialog
  (`fountain_preflight`, `POST /api/a2a/fountain/preflight`) compares the
  `${VAR}`s the agent uses (and `GITHUB_TOKEN` for a GitHub clone) with
  the keys of the run's environment and vault. Fountain lists keys, never
  values. It shows "N variables missing on Fountain", and each missing
  variable has *Copy from this machine to vault V*
  (`fountain_copy`, `POST /api/a2a/fountain/copy`). The copy resolves the
  variable here as wearing does (Infisical through `~/agent-specs`, your
  shell, `gh auth token`) and writes it to the vault once. Nothing is
  copied automatically, a variable the recipe doesn't use can't be copied,
  and no value is shown, logged or kept.
- Running, previewing and copying are this machine's own account's.
