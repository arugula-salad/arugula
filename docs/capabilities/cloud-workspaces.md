---
id: "cloud-workspaces"
title: "Cloud workspaces"
verified_through: "665cdb10ed705245af5afaf697068c829f8504b6"
entry: ["crates/platform/src/hosted/routes.rs", "crates/control/src/wake.rs", "crates/control/src/hosted.rs", "crates/cli/src/cmd/cloud.rs", "crates/arugulad-control-tools/src/cloud.rs", "web/src/platform/pages/hosted-workspace.tsx", "web/src/platform/pages/hosted-manage.tsx"]
paths: ["crates/platform/src/hosted", "crates/platform/src/compute", "crates/platform/src/cloud/mod.rs", "crates/platform/src/db/hosted.rs", "crates/platform/src/db/hosted_usage.rs", "crates/platform/src/db/hosted_snapshots.rs", "crates/platform/src/db/inference.rs", "crates/arugulad-hosted/src", "crates/daemon/src/hosted/mod.rs", "crates/daemon/src/hosted/trust.rs", "crates/control/src/activity.rs", "crates/control-wire/src/hosted.rs", "crates/e2e/src/hosted.rs", "packaging/hosted", "web/src/platform/settings/spend.tsx"]
shared: ["crates/platform/src/router.rs", "crates/platform/src/keys.rs", "crates/platform/src/forge/links.rs", "crates/control/src/relay.rs", "crates/control/src/auth.rs", "crates/control/routes.toml"]
tests: ["crates/platform/src/hosted/tests.rs", "crates/platform/src/hosted/wake_tests.rs", "crates/platform/src/hosted/hold_tests.rs", "crates/platform/src/hosted/usage_tests.rs", "crates/platform/src/hosted/inference_tests.rs", "crates/control/src/hosted_wire.rs", "web/e2e/cloud-workspace-page.spec.ts"]
decisions: ["aru-047", "aru-092", "aru-093", "aru-094", "aru-095", "aru-096", "aru-112"]
---
# Cloud workspaces

## What a person can do
A project's owner or editor whose account is on `ARUGULA_HOSTED_ACCOUNTS` makes a hosted machine from a spec (repos, packages, setup, env names, size), on `/p/:project/workspaces`, with `arugula cloud create` or MCP's `cloud`. Any member lists and opens it, and opening wakes it. It needs a provider key on control (`ARUGULA_E2B_API_KEY`) and a project delegation an owner's device signed; without them the routes answer 404 or the workspace fails saying why.

## Where it runs
Traced by reading the code from `POST /api/projects/{project}/workspaces` to a box's `ready`, and each wake and pause path. Four parts: the platform (routes, worker, hold), the hub (enrollment, relay, wake), the provider (`compute::Substrate`, one adapter `E2b` for E2B and loam), and `arugulad --hosted` in the box (`crates/arugulad-hosted`).

Who: `hosted/routes.rs::changer` refuses a viewer, and refuses a create or rebuild from an account off the list. Callers are a person, a CLI device, or a machine with a live `cloud.drive` grant for its owner (`cloud/mod.rs::CloudDriver`). Anyone outside the project gets 404. A project holds at most 10.

Creating: `routes::create` writes a `hosted_workspaces` row in state `creating`. `hosted/worker.rs` leases the row and takes one step at a time: it makes the box, finding a lost create by its metadata rather than sending it twice, and asks the hub for a one-time enrollment (`HubPort::hosted_enrollment`). The box posts `POST /api/hosted/enroll` (`control/src/hosted.rs`), which signs its certificate under the project's newest live delegation, into `hosted_machines`. A used enrollment or a revoked delegation fails closed, and the box is deleted. Setup runs in a pane on first boot (`arugulad-hosted/src/setup.rs`), and a spec changed since shows as `rebuild_needed`.

Pausing: the box pauses itself at its provider timeout, 10 minutes after it was made, woken or last held unless `ARUGULA_HOSTED_IDLE_TIMEOUT` says otherwise, keeping memory and disk. `arugulad-hosted/src/activity.rs` says busy or idle over the relay, and the leader's `hosted/hold.rs` extends the timeout while busy. An idle or silent box isn't held. Near the provider's runtime cap, `worker.rs::near_cap` pauses and resumes it at an idle moment. No route, CLI command or MCP tool asks for a pause: only tests call `Ask::Pause`, though the worker has the branch, and skips the pause while the daemon says busy.

Waking: `Ask::Wake` is asked by `routes::open`, by `control/src/wake.rs` (through `PlatformPort::hosted_wake` and `hosted/wake.rs`) when someone opens the machine, by a run's `Dispatch`, and by `hold.rs` for a box found paused mid-turn. Summary connections don't wake a box. The worker resumes it, the row is `waking`, and it's `ready` once the daemon dials in. Past `dial_wait` it's `failed` as needing a rebuild, and owners and editors get a push.

What it's billed against, for box time: nothing in this code charges anyone. `routes::create` checks the allowlist and no plan. `billing.rs` reports no sandbox minutes to Stripe. `hosted/usage.rs` meters running intervals into `hosted_usage`, reconciles them with the provider's record in `hosted_usage_provider`, and counts deploy boxes apart; `arugula-platform usage` (`window_portable::usage`) prints it for an operator. `crates/control/tables.toml` says E2B bills by each box's runs, and the provider's key is the platform's.

For inference on a box: `hosted/inference.rs::plan` takes the project's own key first, else the platform's key for a workspace whose maker is on `ARUGULA_CLOUD_ACCOUNTS` when `ARUGULA_HOSTED_PLATFORM_KEYS` covers that provider. Otherwise `turn` refuses with `no_key`. Each agent turn asks first (`POST /api/hosted/turns`). Turns on the platform's key count against per-account caps (2 at once, 100 a day, 120 minutes a day by default), the maker's, or for a thread session its asker's. Keys reach the provider's egress proxy as box rules, never the box; only a digest of the rules is kept. `hosted/broker/` is a separate route with its own payer order, for members and registered clients. `settings/spend.tsx` shows a project's month.

## Compatibility
The template pins the daemon's release (`daemon/src/hosted/mod.rs`), so a box keeps its daemon until it's rebuilt while control moves on. The wire types in `crates/control-wire/src/hosted.rs` are the contract's source (`contract/schema/hosted.json`).

A paused box keeps memory and disk, and only a resume wakes it: no read does, and traffic doesn't. Its daemon redials on resume (`arugulad-hosted/src/resume.rs`).

The flag names `ARUGULA_CLOUD_SECRET_KEY` and `ARUGULA_CLOUD_ACCOUNTS` were kept so a deploy's secrets don't change (`cloud/mod.rs`). A small size leaves a spec's hash alone (`a_small_size_leaves_the_specs_hash_alone`), so existing rows don't turn `rebuild_needed`. A rebuilt box is a new machine, and sessions on it move with `sessions_on_box_changed`.

## Not known
- What this is for, who it's offered to, and what's promised while it's a limited beta: the code only says "limited beta" in a refusal. A question for Jake.
- Whether box time will ever be charged, to whom, or against what plan. Today's code doesn't, and I can't say whether that's intended or only unfinished.
- Who is on `ARUGULA_HOSTED_ACCOUNTS` and `ARUGULA_CLOUD_ACCOUNTS`, and which E2B plan production uses (`ARUGULA_HOSTED_RUNTIME_CAP`), isn't in the code.
- Whether `Ask::Pause` is meant to get a caller, or should go.
- `inference.rs` says the platform's key is set on any request from the box to the provider, so a `claude` in a terminal on an allowlisted maker's box is outside the caps. I read that and didn't test it. Its spend limit is said to be at the provider, which only a person can check.
- aru-047 still names `crates/platform/src/cloud/` as holding the handlers; they're in `hosted/routes.rs` now, and `cloud/mod.rs` keeps only `CloudDriver` and flags. aru-094 says a turn at the cap is logged for Jake; the code also pushes the workspace's maker.
