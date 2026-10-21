PLATFORM PLANES, step 7: runs and deploys

Step 7 of the platform roadmap: **runs and deploys.** CI and production deploys become things a project has.
- **Runs** start on a schedule, from a GitHub event, or by hand, and execute on the project's hosted workspace (step 5).
- **Deploys** stop at a device-signed approval, and are applied by a driver run that holds a scoped credential.

This is the step where the platform joins every estate's blast radius, so its one-way doors matter most. Drafted 2026-10-21 against `main` at 7b13e63e.

## Why

- **The job engine is real and generic.** `crates/platform/src/cloud/worker.rs` leases jobs (`FOR UPDATE SKIP LOCKED` on Postgres) and checkpoints only while it holds the lease.
- **The approval formats are already designed.** S37 (`spikes/s37-team-permissions/`) has device-signed operations, approvals bound to an operation's hash, standing grants, revocation, and a daemon ledger for at-most-once execution. M88 and M89 (#970, #971) were parked in #1002.
- **Deploys live in four places, each with its own approval:**
  - planter's Prod and Go live;
  - redone's plan, approve, apply;
  - studio's `chant run release`;
  - Arugula's own release, run from Jake's shell (#1121 is held).

## Decided (Jake, 2026-10-21)

- **Run logs:**
  - Durable logs go to a bucket the platform can read, **for runs on hosted workspaces only**, kept 30 days. That's a stated exception to aru-055.
  - The live view stays the end-to-end pane.
  - Runs on personal machines are out of this step. If they come later, they keep S37's 64 KB result tail.
- **One standing grant per project for runs.**
  - An owner's device signs "start and instruct on this project's workspaces" once.
  - Scheduled runs start under it with every device asleep. Revoking it stops new runs.
  - **Deploys still need an approval every time.**
- **Drivers:**
  - chant→Fly first, because Arugula's own release is the gate. apps-platform second.
  - **The planter driver moves to step 8,** with planter's absorption, and so does the gate's "a planter app reaches `*.apps.arugula.io`" criterion.
- **Run records:**
  - The platform writes them, using S37's states.
  - The chant driver also writes chant's ledger for releases chant manages, and the platform links to that entry.
  - Tell Alex.
- **Filed natively** as step 6's dogfood (M204). One public GitHub issue links here.
- **From the roadmap, kept:**
  - An agent can plan a deploy and ask for approval, **never approve one,** and that's enforced in code.
  - A deploy goes stale if anything else applied to its environment after it was planned.
  - The approver is a project owner's device.
  - Drivers hold credentials only inside a run: sealed per project, short-lived, **never a kubeconfig.**
  - The event log is append-only, and approvals can be verified by anyone. Don't call it WORM.

## The list

The brief for the agent working this step is #BRIEF.

**Runs:**
1. M207 → M208 (refactor) and M209 (envelope keys).
2. M210 → M211 → M212.
3. M213 and M214 → M215 and M216.

**Deploys** (start after M212):
1. M217 → M218 → M219.
2. M220 → M221 (the gate's release).
3. M222.

**Finishing:** M223 (failure tests) and M224 (cleanup), then M225.

- [ ] #M207 **M207** runs and deploys as decision records
- [ ] #M208 **M208** platform_jobs: one leased job table
- [ ] #M209 **M209** per-project envelope keys for secrets
- [ ] #M210 **M210** run rows: runs, events, artifacts
- [ ] #M211 **M211** the standing project grant
- [ ] #M212 **M212** the execution contract on hosted arugulad
- [ ] #M213 **M213** durable run logs and artifacts
- [ ] #M214 **M214** triggers: manual, schedules, events
- [ ] #M215 **M215** runs on the platform page
- [ ] #M216 **M216** MCP and CLI for runs
- [ ] #M217 **M217** environments and deploys
- [ ] #M218 **M218** the deploy approval
- [ ] #M219 **M219** drivers: a run with a credential
- [ ] #M220 **M220** the chant→Fly driver
- [ ] #M221 **M221** Arugula's own release as a deploy
- [ ] #M222 **M222** the apps-platform driver
- [ ] #M223 **M223** stuck leases, crashes mid-dispatch, and the runbook
- [ ] #M224 **M224** retire the old cloud-run tables
- [ ] #M225 **M225** runs and deploys live, and the gate measured

## Done when (the gate to step 8)

- Arugula's own release runs as a deploy, with Jake approving from the phone. That replaces #1121's held chant operation.
- A scheduled agent run has worked for a week on a hosted workspace.
- The stuck-lease and crash-mid-dispatch tests pass.
- No agent can approve a deploy, and a test proves it.
- The manifest, control-e2e.md and the privacy page say what run logs the platform reads.

**Kill switch:** if nobody but Jake uses deploys after four weeks, keep runs and park the drivers.

Related: #1122–#1263 (steps 1–6), #1002 and S37 (signed approvals), #1121 (the held release), docs/platform-features.md (triggers).
