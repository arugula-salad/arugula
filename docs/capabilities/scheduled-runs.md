---
id: "scheduled-runs"
title: "Scheduled runs"
verified_through: "665cdb10ed705245af5afaf697068c829f8504b6"
entry: ["crates/platform/src/triggers/routes.rs", "crates/platform/src/triggers/read.rs", "crates/platform/src/triggers/agent.rs", "crates/platform/src/run_grant.rs", "crates/platform/src/run_logs/routes.rs", "crates/control/src/hosted_work.rs", "crates/cli/src/cmd/runs.rs", "crates/cli/src/cmd/runs_grant.rs", "crates/arugulad-control-tools/src/runs.rs", "web/src/platform/pages/runs.tsx", "web/src/platform/settings/runs.tsx"]
paths: ["crates/platform/src/triggers/mod.rs", "crates/platform/src/triggers/schedule.rs", "crates/platform/src/triggers/events.rs", "crates/platform/src/work.rs", "crates/platform/src/run_log.rs", "crates/platform/src/run_logs/mod.rs", "crates/platform/src/run_logs/bucket.rs", "crates/platform/src/db/triggers.rs", "crates/platform/src/db/runs.rs", "crates/platform/src/db/run_work.rs", "crates/platform/src/db/run_grants.rs", "crates/platform/src/db/run_logs.rs", "crates/arugulad-work/src/lib.rs", "crates/arugulad-work/src/agent_run.rs", "crates/arugulad-work/src/run_logs.rs", "crates/daemon/src/hosted/runs.rs", "crates/control-wire/src/work.rs", "crates/control-wire/src/runs.rs", "crates/control-wire/src/run_logs.rs", "crates/e2e/src/run.rs", "crates/e2e/src/project_grant.rs", "web/src/platform/runs.ts"]
shared: ["crates/platform/src/router.rs", "crates/platform/src/jobs.rs", "crates/platform/src/inbox.rs", "crates/platform/src/forge/webhook.rs", "crates/platform/src/hosted/wake.rs", "crates/control/routes.toml"]
tests: ["crates/platform/src/triggers/tests.rs", "crates/platform/src/db/triggers_tests.rs", "crates/platform/src/triggers/agent_tests.rs", "crates/platform/src/work_tests.rs", "crates/platform/src/run_grant_tests.rs", "crates/platform/src/run_logs/tests.rs", "crates/platform-chat/src/runs_page_tests.rs", "crates/arugulad/tests/integration/hosted_work.rs", "web/e2e/runs-platform.spec.ts"]
decisions: ["aru-098", "aru-099", "aru-100", "aru-111", "aru-115", "aru-118", "aru-126"]
---
# Scheduled runs

## What a person can do
A project's owner or editor keeps named run specs (a command, or an instruction to Claude Code, on one of the project's hosted workspaces) and fires them by hand, on a cron schedule, or on a linked repo's PR opened, check failed or push. Making or widening one needs the account on `ARUGULA_HOSTED_ACCOUNTS`. A run starts on the box only under the project's live standing grant, which an owner signs with a passkey. Any member reads runs, their logs and their artifacts.

## Where it runs
Traced by reading the code from `POST …/run-specs/{spec}/schedules` to the run's last report. The platform schedules and the box executes.

Schedule: `triggers/routes.rs::add_schedule` checks the caller (`changer`) and the cron (`schedule::check`, at most one tick per 5 minutes), then `Db::add_run_schedule` writes `run_schedules` and a `schedule` job on `platform_jobs`. `in_process::start` calls `work::start`, which registers `schedule::Ticks` and `work::Dispatch` on the job worker, in the hub's process or `arugula-platform`'s.

Firing: `Ticks::step` asks `triggers::Members` whether the schedule's maker may still start runs, then `db/triggers.rs::fire` wins the tick and `start_in` makes the `runs` row under the key `schedule:<id>:<tick>`, names the live `run_grants` row, and queues a `run` job, all in one transaction. Rules take the same path from `forge/webhook.rs` through `triggers/events.rs` and `Db::fire_rule`, under an hourly cap.

Delivery: `Dispatch::step` marks the run ready, and wakes the box if it's paused or hasn't pulled in 30 seconds. The box's daemon (`arugulad-work::start`) pulls `GET /api/hosted/work?takes=run` through the hub (`control/src/hosted_work.rs`, a hosted machine's signature only), which reaches `work::work` and `Db::ready_runs`. One run goes at a time per machine.

On the box: `take` runs `check` (operation hash, this machine, this workspace, the grant through `daemon/src/hosted/runs.rs::allows`), writes the ledger `<state>/work/ID.json` once, and starts a pane in the `runs` session as user `runner` (`pane_command`). Each state change is signed and sent to `POST /api/hosted/work/report`, kept by `work::report` and `Db::move_run` in `run_events`.

The run's surroundings: `env -i` with only `HOME`, `USER`, `LOGNAME`, `SHELL`, `PATH`, `TERM`, `LANG`, `ARUGULA_ARTIFACTS`, and for an agent run the placeholder key with the proxy's CA path (`agent::hosted::env`). It works in a fresh `~runner/work/ID` holding a copy of `directory`, which must be under the daemon's home with no hidden parts. `firewall` resets its TCP to the daemon's and the provider's loopback ports. An agent run is one `claude -p` turn with no MCP config passed, and a turn that ends with background work left fails (`agent_run.rs`). Its turn asks `POST /api/hosted/turns` whose key pays, and a refusal (no key, a cap) fails the run.

Grant: `run_grant.rs` takes v2 grants and v3 revokes only from an owner's passkey; the box checks them again against what it pinned, and keeps revokes in `<state>/run-grant-revokes.json`. No agent signs one.

Logs: `arugulad-work/src/run_logs.rs` sends chunks and artifacts through URLs `run_logs::upload` signs, recorded in `run_log_chunks` and `run_artifacts`, in the `ARUGULA_RUN_LOGS_BUCKET` bucket for 30 days. Without a bucket the log stays in the pane. Members read through `GET /api/runs/{run}/log`, `…/artifacts`, `arugula runs logs` and `read_run` (a machine's agents need `runs.read`).

Who can start one by hand: a person or CLI device through `routes::start`; a machine's agent through `triggers/agent.rs` with `runs.start` or `runs.command`, never past its owner's role; a token's agent through `/api/mcp`.

Reaching a person, as the code stands:
A Doc: no path. The run's process has no Arugula credential and no MCP server given to it. The daemon's `doc` tool is behind the firewall, the session listener (`arugulad-mcp/src/session.rs`) refuses any uid that isn't a session's, a hosted box copies no machine Docs (`arugulad-docs/src/docs/copies.rs`), and `/api/mcp` lists no Doc tool. Project Docs are written by a person, a CLI device or an agent session (`project_docs.rs`, `By`), and nothing starts a session from a schedule.
Chat: no path that a run has. Each run has an object thread that members can post in (`platform-chat/src/chat/objects.rs`), and nothing in the run path posts there. `post_thread` on `/api/mcp` needs an agent token with `threads.post`, and nothing gives a run one.
Notifying: only failure. `inbox::run_failed` makes a `run_failed` entry and a push for the schedule's maker (while a member) and the project's owners, with the push subject to do not disturb and push limits. A run that succeeds tells nobody. A tick that makes no run only sets the schedule's `last_error`.
So a scheduled run's output reaches an account as its log and artifacts, read on `/p/:project/runs`.

## Compatibility
A box runs the daemon of its template until it's rebuilt, so the platform answers old daemons: `Work` and `WorkItem` read new fields with defaults, and a machine says what it takes in `takes`. The contract's schemas are generated from `crates/control-wire` (`contract/schema/work.json`, `runs.json`). The signed formats `arugula op v1` and `arugula run event v1` are shared through `crates/e2e/src/run.rs`.

v1 device-signed grants already kept still go to machines until they expire (`a_v1_grant_kept_before_still_starts_until_revoked`); a new one posted from an old page is refused with `old_grant`. A machine that never heard a revoke honours its grant until it expires. Once a revoke was kept, a missing kept file refuses every start.

A sleeping box doesn't stop a schedule: the tick is the platform's, and the job wakes the box. A tick missed while the platform was down fires once. A run no machine pulls within 15 minutes fails `not_taken`. A run found `running` after a restart with no live pane fails and is never run again.

## Not known
- What runs are for, and what's promised about them: whether a scheduled run is meant to reach Docs, chat or a success notice at all is a question for Jake. A weekly agent can't do those through any path in this code.
- A run has outbound network as far as `firewall` goes (it only touches loopback), so a credential a person plants could work: an agent token in a repo the run copies might reach `post_thread`, and a copied CLI login might pass `Person` on `POST /api/projects/{project}/docs`. Unverified: I read the checks and ran nothing, and the box's egress wasn't traced.
- Whether Claude Code in a run loads an `.mcp.json` from the copied repo depends on Claude Code, not this repo.
- `docs/control.md`, the `start_run` description, `db/triggers.rs` and `triggers/agent.rs` say a run with no grant waits for an approval. In the code `ready_runs` never offers it, no route approves a plain run (`Move::Approved` is only made by deploys), `arugulad-work::grant` refuses one anyway, and `Dispatch::step` fails it `not_taken` after 15 minutes. No test follows it that far; `work_tests.rs` only shows it still pending at first.
- A spec may name a recipe (`agent`), and the route accepts any good name, but `parse_op` refuses anything but `claude`, so such a run fails on the box. Whether that's meant is a question.
- A run's copy comes from what's on the box and nothing in the run path refreshes it; it gets no git credentials. Whether a weekly agent's checkout can stay current wasn't established.
- `hosted_work.rs` runs with `ARUGULA_HOSTED_RUN_USER=""`, so it doesn't exercise `runner` or the firewall. I found no test of them on a real box in the files listed here.
- Whether production sets `ARUGULA_RUN_LOGS_BUCKET`, and who's on `ARUGULA_HOSTED_ACCOUNTS`, isn't in the code.
