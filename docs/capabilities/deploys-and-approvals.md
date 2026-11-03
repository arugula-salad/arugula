---
id: "deploys-and-approvals"
title: "Deploys and approvals"
verified_through: "665cdb10ed705245af5afaf697068c829f8504b6"
entry: ["crates/platform-deploys/src/lib.rs", "crates/platform-deploys/src/deploys.rs", "crates/platform-deploys/src/deploys/agent.rs", "crates/platform-deploys/src/deploys/catalog.rs", "crates/cli/src/cmd/deploys.rs", "crates/arugulad-control-tools/src/deploys.rs", "web/src/platform/pages/new-deploy.tsx", "web/src/platform/pages/deploy.tsx", "web/src/platform/pages/deploys.tsx", "web/src/platform/settings/environments.tsx"]
paths: ["crates/platform-deploys/src/deploys/apply.rs", "crates/platform/src/approvals.rs", "crates/platform/src/drivers", "crates/platform/src/db/deploys.rs", "crates/platform/src/db/deploy_approvals.rs", "crates/platform/src/db/deploy_runs.rs", "crates/arugulad-work/src/deploy.rs", "crates/arugulad-work/src/deploy_chant_fly.mjs", "crates/arugulad-drivers/src", "crates/e2e/src/deploy.rs", "crates/e2e/src/drivers.rs", "crates/control-wire/src/deploys.rs", "crates/control/secrets.toml", "web/src/shared/deploy-approval.ts", "web/src/shared/deploys.ts"]
shared: ["crates/platform/src/work.rs", "crates/platform/src/db/runs.rs", "crates/platform/src/hosted/worker.rs", "crates/platform/src/keys.rs", "crates/platform-api/src/auth.rs", "crates/e2e/src/passkey.rs"]
tests: ["crates/platform-deploys/src/deploys_tests.rs", "crates/platform-deploys/src/deploys/apply_tests.rs", "crates/platform-deploys/src/failure_tests.rs", "crates/arugulad/tests/integration/hosted_work.rs", "web/e2e/deploy-approve.spec.ts", "web/e2e/deploys-waiting.spec.ts"]
decisions: ["aru-099", "aru-101", "aru-102", "aru-103", "aru-104", "aru-126"]
---
# Deploys and approvals

## What a person can do
A project's owner or editor plans a deploy of a commit or artifact to one of the project's environments and asks for approval, on `/p/:project/deploys/new`, with `arugula deploys`, or through an agent's `plan_deploy`. Each owner gets a push, and one approves or denies it with a passkey in a browser or on the phone. Applying needs a control that makes hosted boxes. Some drivers are limited: arugula-release to Arugula's own project, planter's to accounts hosted workspaces allow.

## Where it runs
Traced by reading the code from `POST /api/projects/{project}/deploys` to the deploy's `succeeded` and its box's delete. It runs in the platform (`crates/platform-deploys`, on the base's tables and drivers), and in a box made for that one deploy (`crates/arugulad-work/src/deploy.rs`).

Planning: `deploys.rs::plan_route`, or `deploys/agent.rs::plan` for a machine's agent, reaches `plan`. It finds the environment and its driver (`crates/platform/src/drivers`), which makes the plan's digest: a plan a caller supplies must match it. `Db::plan_deploy` writes the `deploys` row and a `planned` event in `deploy_events`, a hash chain per environment that refuses updates.

Asking: `deploys.rs::request` records `requested` and `push_owners` pushes each owner through the hub. Do not disturb can hold a push back, and `GET /api/deploys/waiting` and the inbox list what's waiting. The newest request makes older open deploys `stale`, and another deploy's apply does too (`Db::may_approve`).

Approving: `web/src/platform/pages/deploy.tsx` checks the operation's bytes (`shared/deploy-approval.ts`), reads `GET /api/deploys/{id}/can-decide`, and asks for control's passkey. `POST /api/deploys/{id}/approve` or `deny` takes `BrowserPerson` (`platform-api/src/auth.rs`): a machine's signature is refused as `agent`, and a CLI login as `not_a_device`. `approvals.rs::verify` checks the expiry (7 days at most), the owner's role in the roster read now, the operation's hash, and the passkey assertion against the account's published log. It's the only maker of a `VerifiedApproval`. `Db::record_deploy_approval` writes `deploy_approvals`, the `approved` event and the `deploy` job in one transaction.

Applying: `deploys/apply.rs`'s `Apply` job checks the expiry, the role and the passkey again, makes a hosted workspace of purpose `deploy` (`add_deploy_box`), and waits for the hosted worker to bring it up (10 minutes at most). Then it makes the run (`make_deploy_run`) and approves it (`approve_run`, the only place outside tests that makes `Move::Approved`). `work::Dispatch` carries it, and only a machine that says `takes=deploy`, which only a deploy box does, gets it.

On the box: `arugulad-work/src/deploy.rs::check` verifies the approval against the owners, passkey logs and origins it pinned (`arugula_e2e::deploy::check_apply`) and refuses otherwise. `apply` reports `running`, which `apply.rs::reported` turns into the deploy's `started`, refusing a stale or expired one. Only then it asks `POST /api/hosted/work/secrets`, answered once per run (`Db::release_deploy_secrets`), with short-lived tokens minted then (`drivers/mint.rs`). The driver runs as `runner`, outside any pane, with its secrets on a pipe, and its output is redacted before it reaches a pane.

What applies it: the driver named by the environment (`db/deploys.rs::DRIVERS`). `chant-fly` runs `deploy_chant_fly.mjs`. `apps-platform`, `arugula-release` and the four `planter-*` run as `arugulad _…` subcommands (`crates/arugulad-drivers`). A fake one is for tests and testnet only.

Ending: the run's end ends the deploy. `Driver::verify` checks what the box reported, giving `succeeded`, else `failed`, which pages the owners once with the run's log. The job deletes the box whatever happened, a run still going after 2 hours is failed, and a reaper is the backstop.

Who: reading is any member. An owner makes environments, and sets their secrets from a browser only (`set_secret` takes `BrowserPerson`). An agent plans and asks with its owner's `deploys.plan` grant, asked on every call, never past its owner's role. `EventKind::allows` refuses an approval from any actor, and `/api/mcp` has no approving tool. The standing run grant doesn't cover deploys: `arugulad-work::check` sends a deploy's run to `deploy::check`, never `grant`.

## Compatibility
The bytes an approval covers are fixed formats, `arugula deploy op v1` and `arugula deploy approval v2`, written the same in `crates/e2e/src/deploy.rs` and `web/src/shared/deploy-approval.ts`; that file says the first lines are pinned in `crates/e2e/src/frozen.rs`. Changing them breaks stored approvals.

v1 approvals, with a device's signature, still read and verify, and one recorded before the switch still applies within its life (`a_v1_approval_from_before_the_switch_still_applies`, `an_old_v1_approval_still_verifies_on_the_deploy_page`). A page that sends a device signature is told to reload (`old_approval`).

`deploy_events` rows are never rewritten: a deleted account's events keep its id, because changing one breaks the chain (`db/deploys.rs`). The hub's passkey log is kept too (`crates/control/tables.toml`).

Machines asleep: after an approval nothing depends on a person's device, since the job makes its own box. Without the hub's answer, a box goes by the passkey log it pinned (`work::work`), and takes only removals from the one it's sent. A deploy's box is made from the release's template (`apply.rs`).

## Not known
- What's promised about deploys beyond the decisions, and which real environments exist, isn't in the code. `crates/control/secrets.toml` lists only those code knows by name.
- AGENTS.md says approval rules, plans and stale checks belong to chant, with a row on #1988 for anything built here first. Whether this feature moves to chant, and when, is a question for Jake.
- aru-126 says the approval is tamper-evident against the operator, not tamper-proof: the platform holds the secrets and enrolls the box. I can't say whether people using this are told so.
- aru-102 says a deploy applies on the project's hosted workspace; the code makes a fresh box per deploy, and `work::work` doesn't offer a deploy's run to a machine that doesn't take deploys. aru-101 and aru-126 mention the fresh box, and `apply.rs` cites Jake for it, but aru-102's text wasn't changed.
- aru-104 says planter's Prod and Go live keep their own approval until a later step. Planter's drivers are now in `drivers/`, and go through this approval in the code I read; I didn't establish whether anything of planter's own approval is left elsewhere.
- `docs/control.md` and the comment in `new-deploy.tsx` say the deploy's card opens control's page to approve. `deploy.tsx` and `deploy-approve.spec.ts` show the platform page approves in place, with control's `#deploy=` dialog as the fallback. `db/deploys.rs` still says the verifier checks an owner device's signature; `approvals.rs` checks a passkey.
- The related-origins fetch production's passkey prompt depends on (control's `/.well-known/webauthn`) isn't run by `deploy-approve.spec.ts`, which says so. Whether it works in a given browser in production can't be told from here.
- `hosted_work.rs` drives the box half with `ARUGULA_HOSTED_RUN_USER=""` and a fake control, so the real `runner` user and hardened sudo weren't shown running by the tests I read.
