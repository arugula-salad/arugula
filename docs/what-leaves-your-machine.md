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
- **Updating**, only when you ask (`arugulad update`, or *Update now* in
  the web client): downloads `SHA256SUMS` and the archive for this OS and
  architecture from GitHub's releases, so GitHub learns those two
  (`crates/daemon/src/selfupdate.rs`, `fetch` and `target`).
- **Telemetry**, off until the owner says yes: see
  [below](#telemetry-off-until-you-say-yes).
- **Feedback**, only when you or an agent sends it (`arugula feedback`,
  the agents' `feedback` tool): what was written, its kind, who wrote it
  (a person or an agent, and the agent's name and version as its MCP
  client says), the version, OS and architecture, and for an agent its
  last failed Arugula tool call's name and error (never its arguments).
  `POST /api/feedback` on the hosted control (or `ARUGULA_TELEMETRY_URL`),
  signed if the machine is joined to it, else with a random install id
  (telemetry's while that's on, else `feedback.install`) and a proof of
  work. Unsent, it waits in `feedback.queue.json` for up to a week
  (`crates/daemon/src/feedback.rs`, `crates/daemon/src/ops/feedback.rs`).
  Telemetry's answer doesn't gate it ([docs/feedback.md](feedback.md)).
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
  and our own function names), which of our programs it was, when, and
  the version, never the panic's message
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
  public keys), its join proof, the protocol features it understands, and
  a team if one was chosen (`POST /api/join`,
  `crates/control-wire/src/join.rs`, `JoinRequest`;
  `crates/e2e/src/cert.rs`, `Cert`; sent by `join_start` in
  `crates/daemon/src/control.rs`). The name is `arugulad join --name`,
  else the hostname; from Getting started it's the daemon's name
  (`--name`, the tailnet name, else the hostname)
  (`crates/daemon/src/lib.rs`, `crates/daemon/src/setup.rs`).
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

### Your terminals: relayed, and control can read them

Control decides who reaches a joined machine
([aru-141](decisions/aru-141-machines-trust-control-the-hub-s-tokens-replace-device-keys.md)).
Say so plainly: **control can read the terminal traffic it relays, and it
can let anyone, itself included, open a shell on any machine an account
reaches.** So can whoever runs control, breaks into it or steals a
signed-in session.

- **Panes, keystrokes and everything else a client asks the daemon**
  travel between your browser or CLI and the daemon through control's
  relay, or straight to the machine on a direct path. A client gets in with
  a machine token control signs (`crates/control-wire/src/machine_token.rs`;
  `crates/daemon/src/machine_token.rs`, `verify`). The channel is still
  encrypted to the daemon's key, but control is the one that hands that key
  out, so the encryption doesn't keep control out. Control is built to keep
  none of what it relays: it holds it in memory on the way through
  ([docs/control-e2e.md](control-e2e.md#what-control-can-do-to-your-machines)).
  It also sees who connects to which machine, and when.
- **What the machine checks itself:** that the token is signed by control,
  for this machine, within its life (control makes them for five minutes;
  the machine refuses any longer than ten) and not used before; and the
  role it names, so a viewer's token can't type
  (`crates/daemon/src/machine_token.rs`; `crates/daemon/src/e2e.rs`,
  `token_grant`). A deploy's box refuses every token.
- **What the machine keeps for you:** each remote session it accepts, with
  who, the role, relay or direct, and when it opened and closed, in
  `remote-sessions.jsonl` in the state directory
  (`crates/daemon/src/remote_log.rs`). The machine's Settings page shows
  it under Remote sessions, to its owner only (`GET /api/remote-sessions`),
  and `arugulad sharing log` prints it on the machine. An agent another of
  your machines started here for a board (aru-143) is an `agent` line in
  the same file: which machine, which board and issue, which block.
- **Agents started here for a board** need a start ticket control signs
  within the board's grant, checked here as a token is, and of this
  machine's own account (`crates/daemon/src/ladder.rs`).
- **What only the machine can turn off:** typing through control.
  `arugulad sharing remote-input off`, run on the machine, makes everyone
  who comes with a machine token a viewer, a session shared with them
  included, and other people's devices too
  (`crates/proto/src/sharing.rs`, `remote_input`). Its owner still watches
  every session on it from other devices, and types into none. Nothing through control
  can turn it back on. A machine joining your own account is on, so you can
  type into your panes from your phone; one joining a team, or moved into
  one later, is off until you turn it on there. `arugulad join --remote-input on|off` picks either way,
  one set before keeps what it has, and the machine menu says which it is.
- **A CLI's sign-in is a file on its machine:** `arugula login` keeps a
  session of your account in `cli-session.json` in the config dir
  (`crates/cli/src/control.rs`), readable by your user, and so by any agent
  you run there. Whoever holds it reaches your machines as you. Control
  gives it a CLI's kind of session, not a browser's, and refuses it what
  only a browser's page does: approving sign-ins, devices, joins and
  deploys, passkeys, standing run grants and a team's members.
  `arugula logout` ends it.
- **Not through control:** a daemon's own page on the machine or your
  tailnet, and `arugula attach` on the machine, talk to the daemon
  directly. `arugulad leave` takes the machine out of control altogether.
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
  Control keeps chat in plain text, and stores it
  (`crates/control/src/lib.rs`, module docs).
- **An agent's answer to a mention** is how it was routed: which step
  took it, the session's name, and, when no session has the name the
  mention used, up to 10 session and tab names it could have used
  (`crates/daemon/src/agent_mentions.rs`, `deliver`;
  `crates/control-wire/src/forge.rs`, `Routed`). Then how that session's
  turn went: working, read, or dropped (`PUT
  /api/daemon/chat/mention-state`; `crates/control-wire/src/chat.rs`,
  `PutMentionState`). Never the session's output. `agent_mentions: false`
  in `sharing.json` in the state directory, or `arugulad sharing
  agent-mentions off`, turns mentions off (`crates/proto/src/sharing.rs`,
  `agent_mentions`; `crates/daemon/src/args.rs`).

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

### Boards

Boards are on for every account; they left Labs. Once this machine has
joined control, and with control's `boards.write` grant:

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
  through control's relay (`crates/daemon/src/mcp/hosted.rs`,
  `on_workspace`; `crates/daemon/src/peer.rs`), which can read it on the
  way, as it can anything it relays. Control runs the box anyway. Needs the
  `cloud.drive` grant (`crates/platform/src/grants.rs`).

### From the box, to control

A box is a daemon started with `--hosted` (`crates/daemon/src/args.rs`).
These run only there; a person's machine doesn't send them
(`crates/arugulad-work/src/run_logs.rs`, module docs, `may_upload`;
`crates/control-wire/src/work.rs`, module docs).

- **Following control's release:** before it dials, at its start and
  after each wake, `GET /control.json` from its control; when the daemon
  release named there is newer than its own, the same download as
  `arugulad update` from GitHub's releases (`crates/daemon/src/hosted/follow.rs`).
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

- **loam (VM panes, `sandbox-providers`):** the loam API at `--loam-url`,
  with your project key (`--loam-key-file`, else
  `~/.config/arugula/loam-key`), for the sandboxes you make: creating
  them, running commands in them (with the pane's environment variables
  in the request), their files, ports and status
  (`crates/arugulad-labs/src/loam.rs`). Nothing is opened or asked
  without `--loam-url`, the key file and the `sandbox-providers` flag on
  (`crates/daemon/src/lib.rs`, `open_provider`;
  `crates/daemon/src/provider_gate.rs`).
- **Agent delegation (`agent-delegation`):** a prompt to a teammate's or
  your other machine's agent, through control's relay, which can read it on
  the way (`crates/daemon/src/peer.rs`;
  `crates/arugulad-labs/src/agents/delegate.rs`).

## What never leaves

- **Your terminals' contents and keystrokes, to control's storage:** they
  pass through control's relay, which can read them, and control is built
  not to keep them (above). Chat is different: what people and agents post
  there, and the lines they quote from a pane, control stores
  (`crates/control/src/lib.rs`). So is a hosted run's log
  ([From the box](#from-the-box-to-control)).
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
| Agents' feedback | `arugulad sharing agent-feedback off` (`agent_feedback: false` in `sharing.json`), `ARUGULA_FEEDBACK=off`; people can still send | `crates/proto/src/feedback.rs`, `agents_may` |
| Agent mentions | `arugulad sharing agent-mentions off` (`agent_mentions: false` in `sharing.json`) | `crates/proto/src/sharing.rs`, `agent_mentions` |
| Typing through control (machine tokens, aru-141) | `arugulad sharing remote-input off` (`remote_input: false` in `sharing.json`), on the machine only: people who come through control can only watch | `crates/proto/src/sharing.rs`, `remote_input`; `crates/daemon/src/e2e.rs`, `token_grant` |
| Who came (not a switch: always on) | `remote-sessions.jsonl` in the state directory, one line per remote session opened and closed; read it on the machine's Settings page (Remote sessions) or with `arugulad sharing log` | `crates/daemon/src/remote_log.rs` |
| Boards state | `arugulad leave`, or take away the machine's `boards.write` grant (`arugula grants`) | `crates/daemon/src/board_state.rs`, `enabled`; `crates/platform/src/grants.rs` |
| Agents' control tools | the machine's grants (`arugula grants`) | `crates/platform/src/grants.rs` |
| VM panes (loam) | no `--loam-url`, or the `sandbox-providers` flag off | `crates/daemon/src/lib.rs`, `open_provider`; `crates/daemon/src/provider_gate.rs` |
| Code-server download | `--code-server PATH` | `crates/daemon/src/args.rs` |
