Brief: burn down platform step 7 (#TRACKER)

The brief for the agent working **step 7 of the platform roadmap**. Progress is tracked in #TRACKER. This issue says how to do the work. Drafted 2026-10-21 against `main` at 7b13e63e. Re-check it against main before starting.

## The job

Land M207–M225 and meet the tracker's gate. When the step is done:
- A project has runs that start on a schedule, from a GitHub event or by hand, on its hosted workspace, with durable logs and artifacts.
- A project has deploys that stop for an owner device's approval, and are applied by a driver run.
- Arugula's own release ships that way, approved from Jake's phone.

## When to start

- **After step 5's gate.** Runs execute only on hosted workspaces (step 5's M179–M182).
- **These issues are native** (step 6's dogfood). Read and update them only through `arugula issue` and MCP. Log anything that sends you to GitHub in M204's friction log.
- **Order:**
  1. M207 (records).
  2. M208 (refactor) and M209 (envelope keys).
  3. Runs: M210 → M211 → M212 → M213 and M214 → M215 and M216.
  4. Deploys: M217 → M218 → M219 → M220 → M221, then M222.
  5. M223 and M224, then M225.

## Read first

- **The tracker:** the decisions, and the gate.
- **The roadmap doc's Step 7** (Jake's private artifact; ask for the link).
- **`spikes/s37-team-permissions/README.md`:** q3 (signed objects, the standing grant, the approval) and q4 (the lifecycle and the ledger).
- **#1002, #970 and #971:** M88 and M89, as parked.
- **Records:**
  - aru-055 (can't read), and its exceptions;
  - aru-071 to 074;
  - aru-073 (grants);
  - aru-091 (agents in conversations);
  - step 5's records on hosted machines and delegation.
- **Code:**
  - `crates/platform/src/cloud/worker.rs` and `db/cloud.rs` (the leased worker);
  - `crates/platform/src/grants.rs`;
  - step 5's hosted workspace code;
  - step 4's GitHub ingester.
- **For the drivers:** #1121 (the held release operation), the arugula chant workspace, home-cloud's apps-platform repo.

## Decided (Jake, 2026-10-21). Don't reopen these.

- **Run logs** go to a bucket the platform can read, for hosted workspaces only, kept 30 days.
- **One standing grant per project for runs.** Deploys are approved every time, by an owner's device.
- **chant→Fly first, then apps-platform.** planter waits for step 8.
- **The platform writes run records.** The chant driver also writes chant's ledger, and the platform links to it.
- **Agents never approve a deploy.**

## How we work here

**Worktrees:**
- Use one worktree per issue under `.claude/worktrees/`, each with `CARGO_TARGET_DIR=$PWD/target`.
- Export the PATH first: `export MISE_NODE_VERSION=22.23.3 PATH=$HOME/.local/share/mise/installs/node/22.23.3/bin:$HOME/.cargo/bin:$HOME/.local/share/mise/shims:$HOME/.local/bin:$PATH`.

**Testing:**
- Build only your own crates, and run only the tests you wrote or changed. Don't run `gh workflow run` on a branch.
- **Run db tests on both dialects** for M208, M210 and M217.
- **Run testnet** for M211, M212, M214 and M219.
- **Run the full `just e2e`** on the rollup for anything touching the web client or the daemon.

**Screenshots:**
- PRs that change a screen get a Before | After table, shot with `just dev-control` or the platform dev recipe and uploaded to the share bucket.
- Other PRs get a Mermaid diagram.

**Migrations:**
- Add new files in both dialects, and never edit one that has shipped.
- Every table gets a `tables.toml` row, and every route a `routes.toml` row.

**PRs and merging:**
- One PR per issue.
- The repo is squash-only.
- When Jake says merge: roll up, then squash each PR in order, putting `[skip ci]` in the `--body` of all but the last. Then watch main and fix forward.

**Decisions you hit:** report them back to Jake as choices.

## Stop and ask first

- **Creating the log bucket** (M213), and its provider.
- **Moving the master key or any release secret** (M209, M221). Escrow in Infisical first.
- **Any real deploy:** the first Fly test project (M220), Arugula's release (M221), the home-cloud test app (M222).
- **Any release or deploy of Arugula itself.**
  - Before releasing, check that main's check run had no failed jobs.
  - `just release` needs the ssh rewrite: `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0='url.git@github.com:.insteadOf' GIT_CONFIG_VALUE_0='https://github.com/'`.
- **Turning runs and deploys on** in production, and for whom (M225).
- **The phone approval checks** need Jake's phone. Have them ready, then ask for a time.
- **Telling Alex** about the run-records decision (M207). Draft the message for Jake.

Never:
- let an agent's grant, tool call or thread message count as a deploy approval;
- store a kubeconfig;
- write a deploy secret to a workspace's disk or a pane;
- upload run logs from a personal machine;
- rewrite a `deploy_events` row.

## Done when

The tracker's gate is met:
- Arugula's release is a deploy, approved on the phone.
- A week of scheduled runs.
- The failure tests in CI.
- The agent-approval test.
- The manifest and privacy page updated.

Tick the tracker as each issue merges. Close this brief with a short report: what merged, the numbers, and anything Jake has to do.
