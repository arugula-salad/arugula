# What leaves your machine

What `arugulad` sends off the machine it runs on, where to, and the setting
that governs each. Every line names the code it comes from: a file, and the
function, route or type in it. If the code and this page disagree, the code
is right and this page is stale; please say so in an issue.

Three kinds of destination:

- **Arugula control**, the hosted one (`https://control.arugula.io`) or
  your own. Nothing goes to control until the machine joins it.
- **A Cloud workspace's box**, a hosted machine running a daemon of its own,
  which you drive from yours.
- **Others:** GitHub, your browser's push service, Let's Encrypt and
  Cloudflare if you ask for certificates, and providers you set up in Labs.

Paths are from the repository's root. Control's routes are the constants in
`crates/control-wire/src/`.

## Before the machine joins control

A daemon that never joined control sends nothing to Arugula unless you turn
telemetry on. It does reach out to these:

- **Update check:** `GET https://github.com/arugula-salad/arugula/releases/latest`
  with `User-Agent: arugula` and nothing else: no version, no id. 5 s after
  start, then every 12 hours (`crates/daemon/src/update.rs`, `latest` and
  `run`). Off with `--no-update-check` or `ARUGULA_NO_UPDATE_CHECK`
  (`crates/daemon/src/args.rs`), on a build from source, and on a hosted
  box (`crates/daemon/src/hosted/mod.rs`, `headless`). `ARUGULA_UPDATE_URL`
  points it elsewhere.
- **Updating**, only when you ask (`arugulad update`, or *Update* in the web
  client): downloads `SHA256SUMS` and the archive for this OS and
  architecture from GitHub's releases, so GitHub learns those two
  (`crates/daemon/src/selfupdate.rs`, `fetch` and `target`).
- **Telemetry**, off until the owner says yes: see
  [below](#telemetry-off-until-you-say-yes).
- **Web Push**, to the push service a browser subscribed with (Google's,
  Apple's, Mozilla's): each notification, encrypted for that browser
  (aes128gcm), with a signed VAPID token (`crates/daemon/src/push.rs`,
  `deliver` and `vapid_jwt`). The token's `sub` claim is
  `mailto:` and the machine's owner login (`--owner`, else the Tailscale
  login that owns the node), or `arugula@localhost` with none
  (`crates/daemon/src/lib.rs`, where `Push::open` is called), so the push
  service sees that login. Nothing is sent until a browser subscribes.
- **GitHub, for PR and issue blocks:** GitHub's REST API, from this machine,
  as you, with the token `gh auth token` gives
  (`crates/arugulad-forge/src/github.rs`, `GhToken::gh` and `api_for`). The
  token stays in the daemon's memory.
- **A browser block's address:** opening a URL block makes the daemon fetch
  that URL itself (up to 5 redirects) to read its title and whether it may
  be framed (`crates/daemon/src/browser.rs`, `probe`). Whoever runs that
  site sees a request from this machine.
- **An editor block, the first time:** downloads code-server 4.140.0 from
  GitHub and checks its SHA-256
  (`crates/arugulad-editor/src/editor/server.rs`). `--code-server PATH`
  (`ARUGULA_CODE_SERVER`) uses one you have instead
  (`crates/daemon/src/args.rs`). What code-server fetches itself (its
  extensions) is its own.
- **Your other machines, on the tailnet:** `GET <url>/api/host`, with no
  credentials, every 60 s, to each host on your host list
  (`crates/daemon/src/hosts.rs`, `probe_urls`).
- **Your home daemon, with `--sync` or `--sync-live` only:** this machine's
  pane history, to the home daemon you joined it to, never to control
  (`crates/daemon/src/sync.rs`, `push_once`; flags in
  `crates/daemon/src/args.rs`).
- **Certificates for block sites, only with `--block-domain` and
  `--block-acme-cloudflare-token-file`:** Let's Encrypt's ACME API (with
  `--block-acme-email` as the account's contact), Cloudflare's API to
  write the `_acme-challenge` record, and Cloudflare's DNS-over-HTTPS to
  check it (`crates/daemon/src/tls.rs`).
- **Tailscale:** only the local `tailscaled`, over its socket (or its CLI
  on macOS and Windows), never Tailscale's servers
  (`crates/daemon/src/tailscale.rs`).

## Telemetry: off until you say yes

- **Off** on a new install, and while `DO_NOT_TRACK` is set,
  `ARUGULA_TELEMETRY=off`, or `CI` is set (`crates/proto/src/telemetry.rs`,
  `enabled_in`). The answer is `telemetry.json` in the state directory:
  `arugulad telemetry on|off|status`, `arugula telemetry …`, or *Usage and
  crash reports…* in the machine's settings
  ([docs/telemetry.md](telemetry.md)).
- **What a report holds:** a random install id (made when you say yes,
  dropped when you say no), the version, OS, architecture, how it was
  installed, which control it's joined to (none, the hosted one, another),
  and events: opted in, started (as a service or not), and a day's counts
  (`crates/control-wire/src/telemetry.rs`, `Report`, `Event`, `Counts`).
  The counts are of panes opened, the most at once, agents started per
  runtime, blocks opened per kind, clients per surface, views, Labs flags
  on, and errors in the web client (`Counts`).
- **Crashes:** where in our code a panic happened (a `crates/…/file.rs:line`
  and our own function names) and the version, never the panic's message
  (`crates/control-wire/src/telemetry.rs`, `Crash`). They're kept in
  `crashes/` and go with the next report; a no empties it
  (`crates/daemon/src/telemetry.rs`, module docs).
- **Where:** `POST /api/telemetry` on the hosted control, or
  `ARUGULA_TELEMETRY_URL` (`crates/daemon/src/telemetry.rs`, `send`),
  signed with the machine's key if it's joined to that control.

## Once the machine joins control

`arugulad join URL`, or *Getting started*, joins it; `arugulad leave` takes
it off (`crates/daemon/src/control.rs`, module docs). Every request below is
signed with the machine's key (`crates/daemon/src/control.rs`, `cloud_raw`
and `post_signed`).

### The machine itself

- **Joining:** the machine's certificate (its account, device id, name and
  public keys), its join proof, and a team if one was chosen (`POST
  /api/join`, `crates/control-wire/src/join.rs`, `JoinRequest`;
  `crates/e2e/src/cert.rs`, `Cert`; sent by `join_start` in
  `crates/daemon/src/control.rs`). The name is the hostname for
  `arugulad join`, or the daemon's name (`--name`, the tailnet name, else
  the hostname) from Getting started (`crates/daemon/src/lib.rs`,
  `crates/daemon/src/setup.rs`).
- **Every 60 s:** a refresh of the account's certificates, the team and its
  peers, which tells control this daemon's version, OS and architecture
  (`GET /api/daemon/trust?…&version=&os=&arch=`;
  `crates/daemon/src/control.rs`, `refresh`, `REFRESH`).
- **Who may reach it through the relay:** the accounts it's shared with,
  until when read-only links may, and which accounts' daemons may reach its
  agents (`POST /api/daemon/access`; `crates/control-wire/src/team.rs`,
  `AccessList`; `crates/daemon/src/control.rs`, `publish`).
- **The relay socket:** a WebSocket to control that stays open, naming the
  addresses clients can reach this daemon at directly: each `--direct-url`
  (`ARUGULA_DIRECT_URL`) and the tailnet name
  (`crates/daemon/src/control.rs`, `relay_once`; `crates/daemon/src/lib.rs`).
  `--no-relay` leaves it closed (`crates/daemon/src/args.rs`).

### Your terminals: relayed, not readable

- **Panes, keystrokes and everything else a client asks the daemon** travel
  in end-to-end channels (Noise IK) between your device and the daemon;
  control relays bytes it can't read, and the keys it hands out are signed
  by your own devices, which the daemon checks itself
  (`crates/control/src/lib.rs`, module docs;
  `crates/daemon/src/control.rs`, module docs;
  `crates/e2e/src/cert.rs`, `Trust::evaluate`; [docs/control-e2e.md](control-e2e.md)). What control does
  see is who connects to which machine, and when.
- **Push through control** (a device away from the tailnet): each
  notification is encrypted for the device's browser before it goes; control
  sees the push endpoint and ciphertext (`POST /api/daemon/push`;
  `crates/daemon/src/control.rs`, `relay_push`).
- **A shared Doc link:** the Doc is encrypted on this machine with a key
  that lives only in the state directory and the link's fragment; control
  stores ciphertext and its size (`crates/arugulad-docs/src/docs/link.rs`,
  module docs, `push`). Only when you share it.

### Chat

- **Who may read each pane's and session's thread:** account ids, never a
  message (`POST /api/daemon/chat/readers`;
  `crates/control-wire/src/chat.rs`, `ChatReaders`;
  `crates/daemon/src/chat_readers.rs`). Only while control says chat is on
  for the machine's account or team (`flags::control_says(…, "chat")`,
  `crates/proto/src/flags.rs`).
- **What people write in chat** goes from their browser to control, not
  through the daemon (`crates/daemon/src/team_chat.rs`, module docs).
  Control keeps chat in plain text: it's the exception to end-to-end
  (`crates/control/src/lib.rs`, module docs).
- **An agent's chat answer to a mention** is only "done" or why not
  (`crates/daemon/src/agent_mentions.rs`, `deliver`). `agent_mentions: false` in
  `sharing.json` in the state directory turns mentions off
  (`crates/proto/src/sharing.rs`, `agent_mentions`).

### Forges

- **When there's no `gh` login for github.com:** the repo's name, to get a
  read-only token from control's GitHub App (`POST /api/daemon/github/token`;
  `crates/daemon/src/control.rs`, `github_token`;
  `crates/arugulad-forge/src/github.rs`, `GhToken::get`). Your own `gh`
  token is never sent to control.
- **Live updates:** the github.com repos that have open blocks, so control
  can pass on GitHub's webhooks (`forge.watch` on the relay socket;
  `crates/arugulad-forge/src/live.rs`).
- **A repo linked to a control project:** whether it's linked (`GET
  /api/forge/repos/{repo}`), the pinned item's id when a block reads it
  from control's mirror, and drafts of comments and reviews, in plain text,
  until a person sends them (`POST /api/forge/drafts`;
  `crates/arugulad-forge/src/platform.rs`, `ask_link`, `create`;
  `crates/arugulad-forge/src/pin.rs`). Sending a draft goes to GitHub from
  this machine, as you (`crates/arugulad-forge/src/send_op.rs`).
- **The inbox shadow, only with `ARUGULA_INBOX_SHADOW` set:** repo, number,
  why it wants attention and draft ids, never a title or text
  (`POST /api/inbox/shadow`; `crates/arugulad-forge/src/shadow.rs`).

### Boards (Labs: `boards`)

With the `boards` flag on (`crates/proto/src/flags.rs`), and control's
`boards.write` grant:

- **An agent block's state:** working, waiting or idle; a title only for a
  block started on an issue; its branch, short HEAD, whether it's pushed, a
  count of uncommitted files and the diffstat's three numbers. No message
  text, file names or transcript (`PUT /api/daemon/block-state`;
  `crates/daemon/src/board_state.rs`, module docs, `enabled`, `send`).
- **Which issue a block works on:** the issue, the machine and the pane
  (`crates/daemon/src/board_attach.rs`, `attach` and `held`).

### What your agents send

Agents on the machine have MCP tools for what lives in control. Each sends
what the agent writes, in plain text, when the agent calls it: chat posts
and files it attaches (read from this machine), issue titles, bodies and
comments, session prompts and replies, a run's command or instruction,
board and tracker items, a Cloud workspace's spec and setup script, and
search words (`crates/arugulad-control-tools/src/threads.rs`, `issues.rs`,
`agent_sessions.rs`, `runs.rs`, `boards.rs`, `trackers.rs`, `forge_ops.rs`;
`crates/daemon/src/mcp/hosted.rs`, `hosted_create`). To find the project,
some send the GitHub repo of the agent's clone (`GET
/api/daemon/issue-home`; `crates/daemon/src/mcp/issues.rs`,
`chosen_project`).

- Only the owner's agents: one a guest stands behind is refused
  (`crates/arugulad-mcp/src/tools.rs`, `dispatch`; each module's
  `owners_*`).
- Control checks the machine's grant on every call (`threads.post`,
  `issues.write`, `runs.start`, `sessions.start`, `cloud.drive` …;
  `crates/platform/src/grants.rs`), which `arugula grants` changes.
- A read-only MCP token can't call the ones that write
  (`crates/arugulad-mcp/src/tools.rs`, `dispatch`).

## Cloud workspaces

### From your machine

- **Creating one:** its name, the repos' URLs, packages, the setup script,
  the names of the secrets it uses (not their values), and its size (`POST
  /api/projects/{p}/workspaces`; `crates/cli/src/cmd/cloud.rs`;
  `crates/daemon/src/mcp/hosted.rs`, `hosted_create`). Secret values go to
  control when you set them (`arugula cloud secret set`, a `PUT` of the
  value; `crates/cli/src/cmd/cloud.rs`, `secret`), not with the spec.
- **No repo contents and none of your credentials:** the box clones from
  the forge itself with a token control mints for it
  (`crates/arugulad-hosted/src/setup.rs`, `script`). Your `gh` token is only
  read by this machine's forge blocks
  (`crates/arugulad-forge/src/github.rs`); agents on the box get a
  placeholder key, `arugula-brokered`, and the real one is added on the way
  out of the box (`crates/daemon/src/agent/hosted.rs`, module docs).
  *Partly confirmed*: no code we found sends your `~/.claude` or local API
  keys to a box, by search, not by proof.
- **Driving one** (an agent's `cloud` and `read_cloud` tools): the tool
  call, with the prompts, text and paths it holds, goes to the box's daemon
  in an end-to-end channel through control's relay
  (`crates/daemon/src/mcp/hosted.rs`, `on_workspace`;
  `crates/daemon/src/peer.rs`). Control can't read it. Needs the
  `cloud.drive` grant (`crates/platform/src/grants.rs`).

### From the box, to control

A box is a daemon started with `--hosted` (`crates/daemon/src/args.rs`).
These run only there; a person's machine doesn't send them
(`crates/arugulad-work/src/run_logs.rs`, module docs, `may_upload`;
`crates/control-wire/src/work.rs`, module docs).

- **Enrolling, setup and activity:** its new public keys and ticket (`POST
  /api/hosted/enroll`; `crates/arugulad-hosted/src/enroll.rs`); setup's
  step and a fixed reason code, never the setup pane's text (`POST
  /api/hosted/setup`; `crates/arugulad-hosted/src/setup.rs`, module docs);
  busy or idle (`crates/arugulad-hosted/src/activity.rs`).
- **Agent turns:** which agent, when a turn starts and ends (`POST
  /api/hosted/turns`; `crates/daemon/src/agent/hosted.rs`, `Gate`). The
  prompts go to the model's provider, not to control.
- **Transcripts, in plain text:** when an agent block on the box closes, or
  before a rebuild, its text (prompts, the agent's messages and tool calls,
  up to 768 KiB) with its title and directory (`POST
  /api/hosted/transcripts`; `crates/control-wire/src/transcripts.rs`,
  `TranscriptPost`; `crates/daemon/src/hosted/archive.rs`, `send`).
- **Agent sessions:** state, the question it's waiting on, the tool
  running, errors, branch and commits, the diffstat, the agent's last
  message (up to 10 KiB), the start of a prompt typed in the block, and the
  last 4 KiB of a failed finish step's output (`POST
  /api/hosted/work/session`; `crates/control-wire/src/work.rs`,
  `SessionDetail`; `crates/arugulad-work/src/sessions.rs`, `send`).
- **Runs:** each run's state changes and exit code (`POST
  /api/hosted/work/report`), and its pane's output and artifacts, to a
  bucket control signs URLs for, kept 30 days
  (`crates/arugulad-work/src/run_logs.rs`, `put`;
  `crates/control-wire/src/run_logs.rs`, module docs).

## Labs providers

- **Sprites (VM panes):** the Sprites API, with your token, for the VMs you
  make: creating, running commands in them (with the pane's environment
  variables in the request), and their status
  (`crates/arugulad-labs/src/sprites.rs`). It's set up whenever the token
  file exists (`--wisp-token-file`, else `$XDG_DATA_HOME/wisp/token`),
  whatever the `sandbox-providers` flag says
  (`crates/arugulad-labs/src/lib.rs`, `open_provider`;
  `crates/daemon/src/lib.rs`), and only calls it for VM hosts you've made
  (`crates/daemon/src/hosts.rs`, `ask_providers`).
- **Agent delegation (`agent-delegation`):** a prompt to a teammate's or
  your other machine's agent, end to end through control's relay
  (`crates/daemon/src/peer.rs`; `crates/arugulad-labs/src/agents/delegate.rs`).

## What never leaves

- **Your terminals' contents and keystrokes, to control:** only in
  end-to-end channels it relays (above). Chat is the exception, for what
  people and agents post there, and quoted lines (`crates/control/src/lib.rs`).
- **Your `gh` token:** used by this machine to call GitHub, never sent
  (`crates/arugulad-forge/src/github.rs`, `TOKENS`;
  `crates/arugulad-forge/src/send_op.rs`).
- **A shared Doc's key:** only in the state directory and the link
  (`crates/arugulad-docs/src/docs/link.rs`, module docs).
- **Telemetry's content:** counts and crash locations only, never terminal
  contents, file names, commands, prompts or a panic's message
  (`crates/control-wire/src/telemetry.rs`, `Counts`, `Crash`).
- **Run logs from your own machine:** it never gets an upload URL and never
  asks (`crates/arugulad-work/src/run_logs.rs`, module docs).
- **Setup output and leftover secrets from a box:** reason codes only
  (`crates/control-wire/src/hosted.rs`, `SetupReport`;
  `crates/arugulad-hosted/src/setup.rs`, `sealed`).
- **Deploy secrets, from a box:** in memory, and redacted from the pane
  before anything reads it (`crates/arugulad-work/src/deploy.rs`, module
  docs).

## The switches

| What | Off, or on | Where it's read |
| --- | --- | --- |
| Everything to control | `arugulad leave`; never joined | `crates/daemon/src/control.rs` |
| Telemetry | `arugulad telemetry off`, `DO_NOT_TRACK`, `ARUGULA_TELEMETRY=off` | `crates/proto/src/telemetry.rs`, `enabled_in` |
| Update check | `--no-update-check`, `ARUGULA_NO_UPDATE_CHECK` | `crates/daemon/src/args.rs`, `crates/daemon/src/update.rs` |
| Relay socket | `--no-relay` | `crates/daemon/src/args.rs` |
| Chat readers | chat off for the account or team, in control | `crates/daemon/src/chat_readers.rs` |
| Agent mentions | `agent_mentions: false` in `sharing.json` | `crates/proto/src/sharing.rs`, `agent_mentions` |
| Boards state | the `boards` flag (`arugulad flags boards off`) | `crates/daemon/src/board_state.rs`, `enabled` |
| Agents' control tools | the machine's grants (`arugula grants`) | `crates/platform/src/grants.rs` |
| History sync | `--sync`, `--sync-live` (off unless given) | `crates/daemon/src/args.rs`, `crates/daemon/src/sync.rs` |
| Inbox shadow | `ARUGULA_INBOX_SHADOW` (off unless set) | `crates/arugulad-forge/src/shadow.rs` |
| Sprites | remove the token file | `crates/arugulad-labs/src/sprites.rs`, `open` |
| Code-server download | `--code-server PATH` | `crates/daemon/src/args.rs` |
