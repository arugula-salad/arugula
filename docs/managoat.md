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
they reach (`control.cloud`). The daemon calls control's `/api/cloud/...`
routes signed with its own key, and control acts as the machine's account,
the owner. So **anyone who may run things on a machine, a teammate with
run rights or a guest's agent there, can drive the owner's Cloud
workspaces through agents on it**: that was decided (Jake, 2026-10-19), not
overlooked. Changing a Cloud workspace's settings, deleting one, secrets,
the agent library and past tabs stay on control's page, for a person.
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
