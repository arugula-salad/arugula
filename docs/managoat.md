# Managoat (removed)

Managoat was the first backend for Cloud workspaces and cloud agents. It's
gone: Cloud workspaces are hosted workspaces now, `arugulad` in a rented
sandbox attached to a project ([control.md](control.md), "Hosted
workspaces"). This page says what was there and what became of it.

## Cloud workspaces on Managoat (removed in M189 and M190)

From M137 to M189, Arugula control ran Cloud workspaces on a Managoat
account it owned: a Managoat environment, vault and workspace per Cloud
workspace, tabs as Managoat conversations, a library of cloud agents, cloud
secrets sealed per account, and one-shot cloud runs (retired earlier, in
S40). Its only user was Jake, and on 2026-10-23 he chose to delete it all
rather than archive it:

- **Managoat's side.** Every Managoat workspace, vault and environment the
  platform made was deleted first (`arugula-platform managoat-retire
  --execute`, 2 of each); the account is empty.
- **The platform's side.** The code went (the Cloud routes under
  `/api/cloud/…`, the leased worker, the move to hosted workspaces, the
  approval pushes, `managoat-retire`), and so did its 11 tables
  (`cloud_agents`, `cloud_runs`, `cloud_run_secrets`, `cloud_secrets`,
  `cloud_jobs`, `cloud_requests`, `cloud_notifications`, `cloud_events`,
  `cloud_workspaces`, `cloud_workspace_specs`, `cloud_tabs`), dropped by
  platform migration 0030 with nothing archived. Old tabs' transcripts and
  past runs went with them. Transcripts a move had already copied into a
  project's archive (`hosted_transcripts`) stay there.
- **Its flags.** `ARUGULA_MANAGOAT_URL`, `ARUGULA_MANAGOAT_API_KEY`,
  `ARUGULA_MANAGOAT_DELETE_AFTER`, `ARUGULA_CLOUD_MODEL`,
  `ARUGULA_CLOUD_MODELS`, `ARUGULA_CLOUD_RUNTIME` and the retired run caps
  (`ARUGULA_CLOUD_DAILY_RUNS`, `ARUGULA_CLOUD_CONCURRENT_RUNS`,
  `ARUGULA_CLOUD_RUN_MINUTES`) aren't read any more; a deploy that still
  sets them starts and ignores them. `ARUGULA_CLOUD_SECRET_KEY` (the master
  key, M209) and `ARUGULA_CLOUD_ACCOUNTS` (the hosted inference beta, M185)
  stay.
- **The daemon's side.** The `cloud_workspace` block and the Managoat
  paths of MCP's `cloud`, `read_cloud` and `list kind cloud_workspaces`
  went; those tools serve hosted workspaces (`hw_…`) only.
- **The web client.** The Cloud workspace pages, the cloud agent library,
  past tabs and runs, cloud secrets and the move dialog went. A project's
  hosted workspaces are on the platform page at `/p/:project/workspaces`.

A build from before migration 0030 can't start on a database that ran it
(the migration isn't additive).

## The daemon's own Managoat integration (removed 2026-10-18)

The daemon's own Managoat integration (#942: its sign-in, the account
stream, the account view and its catalog, recipes run on Managoat, and
views of a conversation's sandbox) was removed on 2026-10-18 ("Daemon-side
Managoat is removed" in [DECISIONS.md](../DECISIONS.md)). Saved local
Managoat agent blocks remain readable as stopped transcripts. They cannot
send prompts, resume, approve tools, wake, or terminate remote
conversations. A saved account view, or a diff or file view of a sandbox,
is dropped when the daemon restores the layout. The old `/api/fountain/…`
routes, `arugula managoat` and MCP's Managoat kinds are gone;
`--managoat-account-stream` still parses, so an old service file starts,
and only logs that it does nothing. Existing credential files are left
untouched.
