# Managed cloud agents

Arugula control owns the Managoat integration. Users open **Cloud
workspaces** and **Cloud agents** in their Arugula account; they do not
need a Managoat account, login, or model-provider credential. Managoat is
an implementation dependency of control, not a connection configured on
each user's machine.

## Cloud workspaces

A Cloud workspace is a session in the shell backed by a Managoat
workspace: one machine in the cloud with your repositories on it, set up
once. Its tabs are agent conversations on that machine, and they share
its disk. Control keeps Cloud workspaces and their tabs; nothing about
them is remembered in the browser.

**Making one.** **+ Cloud workspace** in the sidebar (or **New Cloud
workspace** in the list) takes a name, up to ten repositories (each an
HTTPS URL with an optional branch or tag and a credential from your
secrets), apt and npm packages, a setup script, and the secrets its agents
may use. A repository is cloned to `/home/sprite/<repository name>` unless
it names another absolute mount. For each Cloud workspace control makes
its own Managoat environment (what the machine is built from), vault (its
secrets) and workspace, named after the Cloud workspace's id. Nothing is
shared between Cloud workspaces.

**Its state** shows in the sidebar and the header: *Setting up* while
packages install, repositories clone and the setup script runs, with the
step it's on (*Setting up · packages*); *Ready*;
*Parked* while Managoat has put an idle machine to sleep (a prompt to any
tab wakes it); *Failed*, with Managoat's failing step and the end of what
it printed; and *Rebuild needed* when what it was built from has changed
since. Control polls the workspace for these; a restart doesn't lose them.
**Setup log** on the workspace's page (and *Show setup log* on a failed
one) reads what the last setup printed, step by step, as Managoat kept
and redacted it; it's read when opened, never polled, and never wakes a
parked machine. Both need Managoat v0.39.0.

**Tabs.** **+ Agent** opens a tab: a cloud agent (curated, or one of
yours), a runtime and model from the list control offers, the directory it
starts in (a repository's mount by default, or another directory under
one, or under `/home/sprite`), and an optional first prompt. A tab opened
while the Cloud workspace is setting up keeps its first prompt until setup
is done, and says so. Each tab gets its own Managoat agent, made from the
agent definition as it was when the tab opened, with every tool call
asking first. Approvals and questions show in the tab, in the bell's
drawer and the swarm, and as a push; once answered they say how. Closing
a tab ends its conversation and deletes its agent; its transcript stays,
read only, under *Past tabs* on the Cloud workspaces page, and so do the
tabs of a deleted Cloud workspace, until the account is deleted. A tab
whose conversation failed (a first prompt that ran a setup that failed
ends that conversation in Managoat) starts a new conversation on its
next prompt, with the old transcript kept above.

**New agent → Run on: Cloud** opens an agent as a tab in one of your Cloud
workspaces (the one shown, or the newest), or in a new one made from the
repository you give it.

**Delete** (in the header) names the tabs it ends, then removes the
workspace, the vault and the environment, in that order, surviving a
restart part way. Everything on the machine's disk goes: commit and push
what you want to keep first.

Control sets no limits on Cloud workspaces: no count, no live-turn cap,
no turn deadline and no idle delete. Managoat parks idle machines.

**From agents (#1094).** An agent on a machine joined to control drives
its owner's Cloud workspaces through the `arugula` MCP server: `list`
kinds `cloud_workspaces`, `cloud_tabs` and `cloud_agents`; `read_cloud`
(a tab's transcript and requests, the machine's files, a file, the diff,
git status and the setup log); `cloud` (create one, open a tab, prompt it
and wait for its turn, interrupt, answer, close a tab, rebuild, retry);
and `wait` with `cloud_workspace` and `tab`. `capabilities` says whether
they reach (`control.cloud`). Without a `path`, `read_cloud` kinds
`git_status` and `diff` read the workspace's first repository (its mount,
as built), and the summary names the others; kind `files` lists the
machine's home.

`show` kind `cloud_workspace` (#1117), with `cloud_workspace` and
optionally `tab`, puts one in front of the user as a **Cloud workspace
block** beside the agent's pane. The page can't be framed, so the block
isn't a frame: while someone looks at it, the daemon reads the workspace
and its tabs from control (as above, every 3 seconds) and the block draws
its overview: status, repositories and tabs. A tab's transcript, its
approvals and prompt, and the workspace's Changes are the platform page's
(M155: the daemon's web bundle holds none of its views), so each tab and
*Open* link there, or to control's page, whose old `#cloud-ws=` link goes
on to the platform page, when the page showing the block doesn't know it.
A viewer of the session sees the status and tabs only. The block's `cloud`
method, which the daemon sends to control as the machine and which reaches
only that workspace's routes (and the agent and model lists), is what
older web bundles' views called. The daemon calls control's `/api/cloud/...`
routes signed with its own key, and control acts as the machine's account,
the owner. So **anyone who may run things on a machine, a teammate with
run rights or a guest's agent there, can drive the owner's Cloud
workspaces through agents on it**: that was decided (Jake, 2026-10-19), not
overlooked. Changing a Cloud workspace's settings, deleting one, secrets,
the agent library and past tabs stay on the platform page, for a person.
Not joined, or with a control that has no Cloud workspaces, the tools say
so.

## Secrets and privacy

Cloud prompts, results, and selected secrets are processed by control and
its backend. They are separate from Arugula's encrypted terminal relay.
Secrets are write-only in the UI and encrypted, bound to the account, in
control's database. The secrets a Cloud workspace names (its repository
credentials and the others it lists) are copied to its vault when it's
made. Model-provider credentials belong to the platform.

Account deletion starts deleting the account's Cloud workspaces (their
tabs' agents, then their Managoat workspace, vault and environment) and
refuses until that's done, and until any leftover cloud run has finished
stopping. Then it erases the account's cloud records.

## Past runs

Cloud runs (one agent in a fresh checkout per run, with its changes saved
at the end) were retired for Cloud workspaces (S40, #1076). Nothing starts,
cancels or answers a run any more. Past runs stay readable: **Past runs**
in Cloud agents opens each one read only, with its transcript, its saved
changes, and **Download results and transcript**. Panes of runs that a
browser remembered are forgotten on load.

A run that a deploy finds unfinished is stopped: a create whose response
was lost is found first, the transcript is read to its end, then the
conversation is ended and its agent and environment are deleted. It reads
*cancelled*, and says why. A run that was in recovery because its changes
couldn't be saved completely (a truncated diff or binary files) is left
as it is, with its environment, for an operator: control logs it once a
day. Inspect it before removing its provider resources by hand; that
environment may hold the only copy of a user's changes.

## Existing Managoat users

Use **Import** in Cloud agents to import agent definitions from an existing
Managoat account. The old API key is used transiently for that request and
is not saved as a platform credential. Review the imported definition and
provide any required secrets in Arugula. Existing remote conversations
remain in the old account; importing definitions does not transfer them.

The daemon's own Managoat integration (#942: its sign-in, the account
stream, the account view and its catalog, recipes run on Managoat, and
views of a conversation's sandbox) was removed on 2026-10-18. Saved local
Managoat agent blocks remain readable as stopped transcripts. They cannot
send prompts, resume, approve tools, wake, or terminate remote
conversations. A saved account view, or a diff or file view of a sandbox,
is dropped when the daemon restores the layout. The old `/api/fountain/…`
routes, `arugula managoat` and MCP's Managoat kinds are gone (MCP names
Cloud agents instead); `--managoat-account-stream` still parses, so an old
service file starts, and only logs that it does nothing. Existing
credential files are left untouched for an explicit import.

## Operating the beta

Configure control through deployment secrets and environment variables:

| Variable | Purpose |
|---|---|
| `ARUGULA_MANAGOAT_URL` | Backend origin; defaults to `https://managoat.com`. HTTPS is required except for loopback tests. |
| `ARUGULA_MANAGOAT_API_KEY` | Dedicated platform account credential; enables the worker. |
| `ARUGULA_CLOUD_SECRET_KEY` | 32 random bytes encoded as 64 hex characters, used to encrypt user secrets. Back up separately from the database. |
| `ARUGULA_CLOUD_ACCOUNTS` | Comma-separated Arugula account IDs admitted to the beta. An empty list admits nobody; `*` admits everyone. |
| `ARUGULA_CLOUD_MODELS` | What tabs may run on: comma-separated `runtime:model` pairs (for example `claude:anthropic/<model>,codex:openai/<model>`), the first the default. Each model must match its runtime's prefix (`claude` takes `anthropic/…`, `codex` `openai/…`, `gemini` `google/…`), and each pair needs its platform inference set up on the backend account. |
| `ARUGULA_CLOUD_MODEL` | With `ARUGULA_CLOUD_MODELS` unset, the one model tabs run on; required then. |
| `ARUGULA_CLOUD_RUNTIME` | With `ARUGULA_CLOUD_MODELS` unset, that model's runtime: `claude` (default), `codex`, or `gemini`. |

`ARUGULA_CLOUD_DAILY_RUNS`, `ARUGULA_CLOUD_CONCURRENT_RUNS` and
`ARUGULA_CLOUD_RUN_MINUTES` were the run caps. They still parse, so an old
deploy starts, and only log that they do nothing.

There is no dollar spending limit, and control caps nothing. Platform
inference must be configured on the backend account before admitting
users. Do not put credentials in client builds, repository files, chat, or
command output. Losing the encryption key prevents recovery of stored user
secrets.

Worker stages and provider resource IDs are persisted in control's SQLite
database (`cloud_jobs`, one row per Cloud workspace, tab, or unfinished
past run). Each stage is journalled before control contacts Managoat, so
workers lease jobs, find a create whose response was lost instead of
repeating it, and resume after a process restart. Preserve the database
and encryption key across deployments.

## Moving off Managoat (M189)

Managoat stops being the Cloud workspace backend (#1235): beta Cloud
workspaces are made again as hosted workspaces, and what's left on
Managoat is deleted 14 days after the notice.

- **The move.** Each beta Cloud workspace's page offers "Move to the new
  Cloud workspaces" (`crates/platform/src/cloud/moving.rs`). It first lists
  each repo's uncommitted and unpushed work, read through Managoat's
  `git-status` and `diff` (which never wake a parked machine), and offers it
  as a tar: `unpushed.patch` against the repo's upstream (else
  `origin/HEAD`), `uncommitted.patch` against HEAD, and the untracked files.
  Then it makes a hosted workspace in the same project from the spec:
  repositories become its repos (a ref is checked out by its setup
  script), apt packages its packages, npm packages an `npm install -g` in
  its setup script, and environment variables and the cloud secrets it
  names become its secrets (sealed again with the project's key), with the
  secret its repositories cloned with as `GIT_TOKEN`. Mounts, the setup
  timeout and networking don't carry: the dialog says so. Its tabs'
  transcripts are copied into the project's transcript archive, read only.
  The Managoat workspace stays as it is. A move asked twice makes one
  hosted workspace (`cloud_workspaces.moved_to`).
- **Who.** Only the person whose Cloud workspace it is, and only when the
  platform has hosted workspaces and their account is on
  `ARUGULA_HOSTED_ACCOUNTS`.
- **The notice.** `ARUGULA_MANAGOAT_DELETE_AFTER` (a date, `YYYY-MM-DD`)
  on the platform puts "Beta Cloud workspaces are deleted on …" on their
  pages. Unset, nothing says so.
- **The deletion.** `arugula-platform managoat-retire --dry-run` lists every
  Managoat workspace, vault and environment in the platform's account and
  what it would delete; `--execute` deletes those control made (workspaces,
  then vaults, then environments), and `--all` the rest too. It reads
  `ARUGULA_MANAGOAT_URL` and `ARUGULA_MANAGOAT_API_KEY`, and never touches
  the platform's database. Removing the Managoat code and its tables is the
  PR after it.

## Verification

The control cloud tests (`cargo test -p arugula-control cloud`) drive the
worker against an in-process fake Managoat. They cover account isolation,
encrypted secrets, imports, Cloud workspace setup, failure and delete
(including lost create responses and a restart part way), tabs side by
side, approvals and pushes, the retired run routes, and stopping a
leftover run. The cloud browser spec (`web/playwright.cloud.config.ts`)
runs the UI against a real control process and a fake provider. Daemon
regressions verify that saved Managoat transcripts restore without
starting, and that the old routes and blocks are refused.
