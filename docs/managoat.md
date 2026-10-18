# Managed cloud agents

Arugula control owns the Managoat integration. Users open **Cloud agents**
in their Arugula account; they do not need a Managoat account, login, or
model-provider credential. Managoat is an implementation dependency of
control, not a connection configured on each user's machine.

## Running an agent

Open **New agent**, select **Cloud** under **Run on**, and choose a curated
coding or review agent, or save a custom agent with
instructions, tools, skills, and selected secrets. Supply a repository,
revision, and prompt. Each run creates a fresh environment and checkout.
Local uncommitted work is not uploaded automatically.

Cloud runs open as agent panes beside machine tabs, with a Cloud indicator.
They also appear in the swarm; a waiting run opens its approval in the pane.
Runs continue when the browser disconnects. Open panes return after reload;
closing a pane does not cancel its run. Reopen Cloud agents to find history,
definitions, and secrets. Answer approvals, cancel a run, and download saved
results and transcripts from the pane. **Run again** starts a fresh checkout
with the previous task prefilled; it does not apply earlier results automatically. Approval requests wait within the run's deadline;
the deadline does not pause while waiting for a person. Browser push
notifications require an existing notification subscription.

Control saves the transcript and repository changes before terminating the
conversation and deleting its temporary agent and environment. If changes
cannot be preserved completely (for example, a truncated diff or tracked
binary changes), the run remains in recovery and retains its environment
for operator recovery. A destroyed or inaccessible sandbox can prevent
file recovery; the saved result explicitly reports that limitation.

Cloud prompts, results, and selected secrets are processed by control and
its backend. They are separate from Arugula's encrypted terminal relay.
Secrets are write-only in the UI and account-bound encrypted in the database.
A run snapshots the selected secrets, so editing a secret does not change
an already queued run. Model-provider credentials belong to the platform.
Account deletion waits for active cloud runs to finish cleanup (including
operator recovery when needed), then erases the account's cloud records.

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
| `ARUGULA_CLOUD_MODEL` | Required model supported by the platform account. |
| `ARUGULA_CLOUD_RUNTIME` | `claude` (default), `codex`, or `gemini`; must match the model prefix. |
| `ARUGULA_CLOUD_DAILY_RUNS` | Per-account daily admission cap; default 10. |
| `ARUGULA_CLOUD_CONCURRENT_RUNS` | Per-account concurrent admission cap; default 2. |
| `ARUGULA_CLOUD_RUN_MINUTES` | Run deadline including approval waits; default 30. |

These are run and time caps, not a dollar spending limit. Platform inference
must be configured on the backend account before admitting users. Do not
put credentials in client builds, repository files, chat, or command output.
Losing the encryption key prevents recovery of stored user secrets.

Worker stages and provider resource IDs are persisted in control's SQLite
database. Workers lease runs, reconcile uncertain creation responses, and
resume after a process restart. Preserve the database and encryption key
across deployments. Inspect runs in recovery before manually removing
provider resources; those environments may hold the only remaining copy
of a user's changes.

## Verification

The control cloud tests cover account isolation, limits, encrypted secrets,
imports, durable execution, approvals, result capture, and cleanup. The
cloud browser spec exercises the UI against a real control process and a
fake provider. Daemon regressions verify that saved Managoat transcripts
restore without starting, and that the old routes and blocks are refused.

The ignored `cloud::worker::tests::live_subscription_run` test exercises a
real backend account. Set `ARUGULA_MANAGOAT_API_KEY`, `ARUGULA_CLOUD_MODEL`,
`ARUGULA_CLOUD_RUNTIME`, and `ARUGULA_LIVE_TEST_DB` through a private test
environment. Run it through the repository's suite lock. Inspect any
requested approval before answering it; retain the test database to resume
or recover a partially completed run.
