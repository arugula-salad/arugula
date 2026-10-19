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

Since then (merged on Managoat's main 2026-10-19, **not released yet**;
managoat.com gets them with the next Managoat release):

- `POST /api/workspaces/:id/rebuild` (#2986, #3018), with an optional
  `{"interrupt": true}`. It answers `202` with `status: "provisioning"`. The
  disk is wiped, and conversations keep their transcripts and lose their
  runtime sessions. Refusals: `409 turns_running` (with `conversation_ids`),
  `409 rebuild_in_progress` (while `provisioning`), `422
  sandbox_provider_disabled`, and `503 fleet_full` / `sandbox_unavailable`
  (nothing changed, so try again). It's **allowed on a `failed`
  workspace**, which makes it the retry too.
- `revision`, `built_from_revision`, `stale` and `changed_fields` (names
  only, never values) on the workspace (#2987, #3012). An environment edit
  that moves `revision` sends `workspace.status`. A workspace also has a
  `sandbox_status` (`pending`, `starting`, `ready`, `suspended`, or `null`)
  beside `status`.
- A conversation's `cwd` may be a repository the environment clones outside
  `/home/sprite`, for example `/work/app` (#3011).

Still open, and nothing waits on either:

| Gap | Issue | Meanwhile |
|---|---|---|
| Setup output while it runs, and kept afterwards | managoat/managoat#3009 | "Setting up…" plus the failure tail |
| A separate retry call | managoat/managoat#3013 | **Retry** calls rebuild on a `failed` workspace, so #3013 is a nicety |

## Decided (Jake, 2026-10-18 and 2026-10-19; in #1039)

- Rebuild waits for managoat/managoat#2986, with no delete-and-recreate
  fallback.
- Each workspace gets a vault.
- Control sets no limits: no workspace cap, no live-turn cap, no turn
  deadline, no idle delete.
- Cloud runs are retired: **New agent → Cloud** goes to a workspace, and past
  results stay readable.
- Start now. (The fingerprint stopgap is gone: #2987 shipped before the
  build started, and Managoat's `stale` and `changed_fields` drive **Rebuild
  needed**.)
- Setup shows its status and the failure tail. #3009 is adopted when it
  lands.
- New shell only.
- Plan decisions reviewed 2026-10-19. Jake changed three of them: a runtime
  and model chosen per tab (4), a separate Managoat retry issue (6, now
  managoat/managoat#3013), and no diff saving before a rebuild (9).
- UI name: **Cloud workspace**. Code names: `cloud_workspace` /
  `CloudWorkspace`, never a bare `Workspace`. The shell already uses
  `useWorkspaceKeys`, `arugula.workspace.*` and `BlockType::Workspace`.

## Decisions this plan makes (reviewed by Jake, 2026-10-19)

1. **Control owns one environment and one vault per workspace**, named
   after the workspace id and tagged `metadata.arugula_workspace`. That lets
   `find` reconcile a lost create the same way it does for runs. Nothing is
   shared between workspaces, so editing one can't change another's next
   rebuild.
2. **The saved spec** is `{name, repositories[{url, ref, mount, secret}],
   packages{apt, …}, setup_script, setup_timeout_seconds, env_vars,
   networking}`. Repositories mount at `/home/sprite/<repo name>` by default.
   **Rebuild needed** is Managoat's `stale`, and *which* fields is its
   `changed_fields`. Managoat sends names only, so control keeps
   `built_spec` (the saved spec as of the revision that last reached
   `ready`, matched on `built_from_revision`) to show before and after
   values. There's no fingerprint.
3. **Status comes from polling, not the account stream.** The worker polls
   `GET /api/workspaces/:id` for live workspaces, as it polls run events
   today. That's one mechanism, and it survives restarts. The account event
   stream can come later.
4. **Each tab gets its own Managoat agent**, created from the cloud agent
   definition snapshotted when the tab opens, with `permission_policy: ask`
   as runs have, and deleted when the tab closes. **The runtime and model
   are chosen per tab** (changed by Jake). Control takes
   `ARUGULA_CLOUD_MODELS`, a list of `runtime:model` pairs (for example
   `claude:claude-opus-5-5,codex:<model>,gemini:<model>`), each checked
   against the existing rule that the runtime matches the model's prefix.
   **+ Agent** offers the list, and the first entry is the default. When the
   list is unset, `ARUGULA_CLOUD_MODEL` and `ARUGULA_CLOUD_RUNTIME` make a
   one-entry list, so today's deploy keeps working. Each pair needs its
   platform inference set up on the Managoat account before it's listed.
5. **A tab's `cwd`** defaults to the first repository's mount, and can be
   any other mount (which may be outside `/home/sprite`, #3011) or a path
   under `/home/sprite`. The mounts are the built settings' (`built_spec`;
   the saved ones only before the first build): a repository saved since
   isn't on the disk until a rebuild (changed by Jake).
6. **Failed setup.** Managoat retries a failed setup only on the next
   prompt into one of the workspace's conversations. Until
   managoat/managoat#3013 lands, **Retry** on a failed workspace calls
   `POST /api/workspaces/:id/rebuild`, which Managoat allows on a `failed`
   workspace. A failed workspace has no machine, so there's no disk to wipe
   and the warning isn't shown. If #3013 lands, Retry moves to it.
7. **Tabs and workspaces live in control's database**, not localStorage. The
   open tab list is the workspace's live conversations.
8. **The event pump is generalised, not copied.** `cloud_events`,
   `cloud_requests` and `cloud_notifications` key on a conversation
   (`run_…` or `cwc_…`) instead of only a run, so approvals, push and the
   attention dot work the same for tabs.
9. **Nothing is saved before a rebuild** (changed by Jake). The confirm
   says plainly that the disk is wiped: uncommitted and untracked work,
   anything outside the repos, and each agent's runtime memory. The user
   commits or pushes first. Transcripts are kept.
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
  built_spec, status, sandbox_status, error, revision, built_from_revision,
  stale, changed_fields, environment, vault, provider_id, created, deleted) and the job rows for them in `cloud_jobs`,
  with a `kind`.
- `worker.rs`: create (environment, then vault and its secrets, then
  workspace), each stage journalled and reconciled after a lost response;
  status polling into `status`, `sandbox_status`, `error` and the revision
  fields, and `built_spec` set when `built_from_revision` reaches `ready`; delete (workspace, then vault,
  then environment), which survives a restart part way through.
- Routes: `GET/POST /api/cloud/workspaces`, `GET/DELETE
  /api/cloud/workspaces/{id}`. Another account's id gets 404 everywhere.
- Account erasure deletes the account's workspaces as it does its runs.

### M115 (#1072): control, tabs (conversations in a workspace)

- `ARUGULA_CLOUD_MODELS` (decision 4), parsed and checked at startup, with
  the one-entry fallback. `GET /api/cloud/models` lists it for **+ Agent**.
- `POST /api/cloud/workspaces/{id}/tabs` `{agent_id, runtime, model, cwd,
  prompt?}` refuses a pair that isn't listed, then creates the agent with
  that runtime and model, then launches with `workspace_id` and `cwd`, without
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
  **+ Agent** (agent definition, runtime and model from the list, `cwd`
  from the mounts, first prompt). Each tab shows its runtime.
- A tab view that reuses `RunDetails`' transcript and approval rendering.
  It's extracted into a shared component rather than copied.
- `shell/context-header.tsx`: status and **Delete** (the confirm names the
  live tabs it ends). The failed state shows `error` and the decision 6
  text. **Retry** arrives in M120 with the rebuild call.
- `shell/attention-drawer.tsx` and `overview.tsx`: tab items next to run
  items.
- The create dialog: name, repositories, packages, setup script, secrets.

### M117 (#1074): settings and pending changes

- `PATCH /api/cloud/workspaces/{id}` saves `saved_spec` and PATCHes the
  environment. Secret changes go to the vault and are live, with no rebuild.
- The response, and the workspace, carry `pending: [{field, before,
  after}]`, built from Managoat's `stale` and `changed_fields` with values
  from `built_spec` and `saved_spec`. `env_vars` values are masked, as
  Managoat does.
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

### M120 (#1077): Rebuild and Retry (after M117)

- **Rebuild** in the header and in Settings. It's refused while a tab is
  mid-turn, unless the user picks **Interrupt and rebuild**. The confirm
  warns that the disk is wiped (decision 9). Control then calls
  `POST /api/workspaces/:id/rebuild` (`interrupt: true` for the interrupt
  choice), maps `409 turns_running` to the mid-turn refusal that names the
  tabs, maps `409 rebuild_in_progress` to "already setting up", and retries
  `503` later. While it waits, the header and Settings say **Rebuild
  waiting for a machine** with **Cancel**
  (`DELETE /api/cloud/workspaces/{id}/rebuild`), which clears it unless the
  worker has taken it to send (`409 rebuild_started`; changed by Jake). It
  shows progress through the status poll. **Retry** on a
  failed workspace is the same call, without the warning. `built_spec` updates when the
  workspace reaches `ready`.
- The live check runs on hosted control after merge (Managoat v0.38.0 has
  rebuild and retry). If #3009 has landed, show the setup log.

Order: M114 → M115 → M116 → M117 → M118. M119 can run after M116, alongside
M117 and M118. M120 follows M117.

## Done when

The acceptance list in #1039 passes on control against managoat.com. That
covers two repos plus `apt: [ffmpeg]` reaching `ready`; a claude tab and a
codex tab sharing the disk; the badge after a settings edit; a rebuild that
keeps the tabs; the failure tail on a broken setup; a parked workspace waking
only on a prompt; delete; account isolation; and **New agent → Cloud** with
past results still readable. `docs/managoat.md` describes Cloud workspaces.

## Left out

Sharing a workspace with a team or another person (Managoat refuses it; see
#1002). Copying or forking a workspace. Uploading local uncommitted work.
Changing a workspace's vault after create. Isolation between tabs in one
workspace. Saving diffs before a rebuild. The account event stream instead of polling.
