# #1039: Cloud workspaces

Planned against `main` at affef44a (v0.46.5). Numbers: S40 (this plan) and
M114–M120 (#1071–#1077), tracker #1078. S39 holds M107–M113 (#1030).

The brief is #1039. In it, a Cloud workspace is a session in the new shell
backed by a Managoat workspace (ADR 0097, shipped in Managoat v0.37.x and live
on managoat.com). Its tabs are agent conversations on one sandbox. A Settings
panel edits what the sandbox is built from, and the session says **Rebuild
needed** when the saved settings and the built ones differ.

## What's there today

Control runs Cloud agents as one-shot **runs**:

- `crates/control/src/cloud/worker.rs` (623 lines) is a checkpointed state
  machine over `cloud_jobs`, leased (`lease`, `lease_until`), with each stage
  journalled in `Execution` before control contacts Managoat. Per run it
  creates an environment (one repository at `/home/sprite/repo`), uploads the
  run's secrets to the environment, creates an agent from the definition
  (`permission_policy: ask`), launches one conversation with
  `sandbox_api_access: "none"`, and pumps its events into `cloud_events`.
  It saves the diff and the transcript, then terminates the conversation and
  deletes the agent and the environment.
- The tests in `worker.rs` drive it against an in-process `Fake` provider
  (an axum fallback that records calls). They cover a lost create response,
  durable cancel, binary and truncated diffs, deadlines and recovery.
  `live_subscription_run` is the ignored live test.
- `crates/control/src/db/cloud.rs` (797 lines) has the tables
  `cloud_agents`, `cloud_runs`, `cloud_secrets`, `cloud_run_secrets`,
  `cloud_jobs`, `cloud_requests`, `cloud_notifications` and `cloud_events`.
  `RunLimits` holds the daily, concurrent and duration caps.
- `crates/control/src/cloud/provider.rs` (211 lines) has generic
  `create`/`find`/`remove` over `/api/{collection}` (found by
  `metadata.arugula_run`), `launch`, `events`, `conversation`, `answer`,
  `interrupt` and `terminate`. It has no vault, workspace or PATCH calls.
- Routes live under `/api/cloud/…` in `crates/control/src/main.rs:504-514`
  (secrets, runs, requests, cancel, events, import, agents).
- Control has one platform model and runtime (`ARUGULA_CLOUD_MODEL`,
  `ARUGULA_CLOUD_RUNTIME`).
- Web: `web/src/cloud.ts` keeps the opened runs in localStorage
  (`arugula.cloud-panes.<account>`), polls `runs` every 3 s, and turns runs
  into `FleetPane`s with `cloudRun` set. `web/src/ui/cloud.tsx` has
  `CloudPanel` (the launch form and the runs), `RunDetails` (transcript,
  approvals, results) and `CloudTabs`.
- In the new shell, which has been the only shell since M113 (#1047), the
  route switch is in `app.tsx`, `CloudTabs` is in `shell/tab-strip.tsx`, and
  `cloudRun` items appear in `shell/attention-drawer.tsx` and
  `shell/overview.tsx`. The sidebar leaves cloud with `cloudPanes.local()`.
  **New agent → Run on: Cloud** (`ui/agent-dialog.tsx`) opens
  `CloudPanel launchOnly`.
- `web/e2e/cloud.spec.ts` runs a real control process against a fake provider
  (a node `createServer`), under `web/playwright.cloud.config.ts`.

Managoat now gives us (manual: *About workspaces*):

- `/api/workspaces` (create, list, show, delete). `environment_id`, an
  optional `vault_id` and `name` are fixed at create. Statuses are
  `provisioning`, `ready` and `failed`, with `error` (the failing step and
  the redacted tail).
- `workspace_id` and `cwd` on `POST /api/conversations`.
- The four file reads at `/api/workspaces/:id/{files,file,diff,git-status}`.
- Parking when idle, waking on a prompt, and a lazy rebuild after a lost
  machine.
- A retry or rebuild runs the environment as it is at that moment.
- Vault edits reach live conversations through the broker.

What Managoat doesn't give us yet:

| Gap | Issue | Until then |
|---|---|---|
| A rebuild call | managoat/managoat#2986 | No Rebuild button (decided). M120 waits for it |
| The revision a sandbox was built from | managoat/managoat#2987 | Control's spec fingerprint (decided 2026-10-19) |
| Setup output while it runs, and kept afterwards | managoat/managoat#3009 | "Setting up…" plus the failure tail (decided) |
| A retry for a **failed** setup without a prompt | none filed yet; see decision 6 | The next prompt in any tab retries (Managoat's behaviour) |

## Decided (Jake, 2026-10-18 and 2026-10-19; in #1039)

- Rebuild waits for managoat/managoat#2986, with no delete-and-recreate
  fallback.
- Each workspace gets a vault.
- Control sets no limits: no workspace cap, no live-turn cap, no turn
  deadline, no idle delete.
- Cloud runs are retired: **New agent → Cloud** goes to a workspace, and past
  results stay readable.
- Start now. **Rebuild needed** comes from control's fingerprint until #2987
  lands.
- Setup shows its status and the failure tail. #3009 is adopted when it
  lands.
- New shell only.
- UI name: **Cloud workspace**. Code names: `cloud_workspace` /
  `CloudWorkspace`, never a bare `Workspace`. The shell already uses
  `useWorkspaceKeys`, `arugula.workspace.*` and `BlockType::Workspace`.

## Decisions this plan makes (change any)

1. **Control owns one environment and one vault per workspace**, named
   after the workspace id and tagged `metadata.arugula_workspace`. That lets
   `find` reconcile a lost create the same way it does for runs. Nothing is
   shared between workspaces, so editing one can't change another's next
   rebuild.
2. **The saved spec** is `{name, repositories[{url, ref, mount, secret}],
   packages{apt, …}, setup_script, setup_timeout_seconds, env_vars,
   networking}`. Repositories mount at `/home/sprite/<repo name>` by default.
   The **fingerprint** is a SHA-256 of the canonical JSON of the
   provision-time fields (everything except `name`). Control stores
   `built_fingerprint` and `built_spec` when a setup reaches `ready`. Pending
   changes are `saved_spec` against `built_spec`, field by field. When #2987
   lands, the revision replaces the fingerprint and `built_spec` stays as the
   source of the diff.
3. **Status comes from polling, not the account stream.** The worker polls
   `GET /api/workspaces/:id` for live workspaces, as it polls run events
   today. That's one mechanism, and it survives restarts. The account event
   stream can come later.
4. **Each tab gets its own Managoat agent**, created from the cloud agent
   definition snapshotted when the tab opens, with `permission_policy: ask`
   as runs have, and deleted when the tab closes. Every tab uses the
   platform's model and runtime. Choosing a runtime per tab is out of this
   track: control has one platform runtime.
5. **A tab's `cwd`** defaults to the first repository's mount, and can be
   any other mount or a path under `/home/sprite`.
6. **Failed setup.** Managoat retries a failed setup only on the next
   prompt into one of the workspace's conversations. Until it has a call for
   that, the failed state says "Fix the settings, then send a prompt in any
   tab to set up again", and with no tabs open it offers **+ Agent**. Ask
   Managoat to let #2986's rebuild call also retry a `failed` workspace (a
   comment on #2986); then **Retry** becomes the rebuild call.
7. **Tabs and workspaces live in control's database**, not localStorage. The
   open tab list is the workspace's live conversations.
8. **The event pump is generalised, not copied.** `cloud_events`,
   `cloud_requests` and `cloud_notifications` key on a conversation
   (`run_…` or `cwc_…`) instead of only a run, so approvals, push and the
   attention dot work the same for tabs.
9. **Before a rebuild, control saves each repository's diff** from
   `/api/workspaces/:id/diff`, under the runs' rule: a truncated diff or one
   with binary changes stops the rebuild and says why.
10. **Retiring runs** leaves `cloud_runs` and its results readable under
    **Past runs**, removes the run worker, the run routes that start or
    cancel anything, and `ARUGULA_CLOUD_DAILY_RUNS`, `_CONCURRENT_RUNS` and
    `_RUN_MINUTES`. The setting still parses and logs that it does nothing,
    the way `--managoat-account-stream` did, so an old deploy starts.

## Milestones

Each milestone is one PR and leaves main releasable. Control milestones
test against the `Fake` provider in `worker.rs` (extended for workspaces,
vaults and PATCH). Web milestones extend `web/e2e/cloud.spec.ts`'s fake.
Per the usual rule, each PR runs its own crate's tests, the full suite runs
on merge, and every UI PR has Before | After shots.

### M114 (#1071): control, cloud workspace records and their lifecycle

- `provider.rs`: `workspaces` create, show and delete. `vaults` create and
  delete, and vault secrets. `PATCH /api/environments/:id`. `find` by
  `metadata.arugula_workspace`.
- `db/cloud.rs`: `cloud_workspaces` (id `cw_…`, account, name, saved_spec,
  built_spec, built_fingerprint, status, error, environment, vault,
  provider_id, created, deleted) and the job rows for them in `cloud_jobs`,
  with a `kind`.
- `worker.rs`: create (environment, then vault and its secrets, then
  workspace), each stage journalled and reconciled after a lost response;
  status polling into `status` and `error`; delete (workspace, then vault,
  then environment), which survives a restart part way through.
- Routes: `GET/POST /api/cloud/workspaces`, `GET/DELETE
  /api/cloud/workspaces/{id}`. Another account's id gets 404 everywhere.
- Account erasure deletes the account's workspaces as it does its runs.

### M115 (#1072): control, tabs (conversations in a workspace)

- `POST /api/cloud/workspaces/{id}/tabs` `{agent_id, cwd, prompt?}` creates
  the agent, then launches with `workspace_id` and `cwd`, without
  `sandbox_api_access: "none"` (Managoat refuses it in a workspace).
  `DELETE …/tabs/{tab}` terminates the conversation and deletes the agent.
- Prompt, interrupt, approve: `POST …/tabs/{tab}/prompt`, `…/interrupt`,
  and the requests routes, keyed on the conversation (decision 8).
- The event pump and push notifications for tab conversations. A tab
  launched while `provisioning` waits with its first prompt (Managoat's
  behaviour) and says so.

### M116 (#1073): web, the Cloud workspace session in the new shell

- `web/src/cloud.ts`: a `cloudWorkspaces` store that polls control. The
  route is `#cloud-ws=cw_…[/tab]`. Tabs come from control, not localStorage.
- `shell/session-sidebar.tsx`: a **Cloud workspaces** group with status
  (setting up, ready, parked, failed, rebuild needed) and **+ Cloud
  workspace**.
- `shell/tab-strip.tsx`: the workspace's tabs, with attention dots, and
  **+ Agent** (agent definition, `cwd` from the mounts, first prompt).
- A tab view that reuses `RunDetails`' transcript and approval rendering.
  It's extracted into a shared component rather than copied.
- `shell/context-header.tsx`: status and **Delete** (the confirm names the
  live tabs it ends). The failed state shows `error` and the decision 6
  text.
- `shell/attention-drawer.tsx` and `overview.tsx`: tab items next to run
  items.
- The create dialog: name, repositories, packages, setup script, secrets.

### M117 (#1074): settings and pending changes

- `PATCH /api/cloud/workspaces/{id}` saves `saved_spec` and PATCHes the
  environment. Secret changes go to the vault and are live, with no rebuild.
- The response, and the workspace, carry `pending: [{field, before,
  after}]` from `saved_spec` against `built_spec`.
- Web: a **Settings** panel (a form over the saved spec, Built vs Saved,
  the last `error`), and the **Rebuild needed · N changes** badge in the
  header and the sidebar.

### M118 (#1075): workspace changes and files

- `GET /api/cloud/workspaces/{id}/diff`, `/git-status`, `/files` and
  `/file`, proxied with the account check. They never wake a parked
  workspace, following Managoat's rule.
- Web: a **Changes** view in the session (per repository diff and git
  status), plus file viewing, over the existing diff rendering in
  `ui/cloud.tsx`'s results.

### M119 (#1076): retire Cloud runs (after M116)

- **New agent → Run on: Cloud** picks a Cloud workspace, or creates one
  with the repository it's given, and opens a tab there.
- Remove the run worker stages, `start_run` and `cancel_run`, the run caps
  and their env vars (decision 10). `CloudPanel` becomes **Past runs**,
  read-only with downloads.
- Drop `arugula.cloud-panes.*` from localStorage on load. Rewrite
  `docs/managoat.md` and update `docs/features.md`.

### M120 (#1077): Rebuild (after M117; blocked on managoat/managoat#2986)

- **Rebuild** in the header and in Settings. It's refused while a tab is
  mid-turn, unless the user picks **Interrupt and rebuild**. Control saves
  diffs first (decision 9), calls the rebuild route, and shows progress
  through the status poll. `built_spec` and the fingerprint update when the
  workspace reaches `ready`.
- If #2987 has landed by then, switch pending changes to Managoat's
  revision. If #3009 has, show the setup log.

Order: M114 → M115 → M116 → M117 → M118. M119 can run after M116, alongside
M117 and M118. M120 waits for #2986.

## Done when

The acceptance list in #1039 passes on control against managoat.com. That
covers two repos plus `apt: [ffmpeg]` reaching `ready`; a claude tab and a
second tab sharing the disk; the badge after a settings edit; a rebuild that
keeps the tabs; the failure tail on a broken setup; a parked workspace waking
only on a prompt; delete; account isolation; and **New agent → Cloud** with
past results still readable. `docs/managoat.md` describes Cloud workspaces.

## Left out

Sharing a workspace with a team or another person (Managoat refuses it; see
#1002). Copying or forking a workspace. Uploading local uncommitted work.
Changing a workspace's vault after create. Isolation between tabs in one
workspace. A runtime per tab. The account event stream instead of polling.
