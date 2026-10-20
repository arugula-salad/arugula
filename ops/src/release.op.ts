import { Op, phase, shell, gate, releasePlan, releaseRecord } from "@intentius/chant/op";

// A release, behind Jake's approval (docs/development.md#releasing). The
// steps are scripts/release's, the ones `just release` runs; the op adds the
// gate in front of the cut:
//
//   Prep     the newest green commit on main and the version it gets, and
//            the notes on main (missing, a draft goes to dist/release/ and
//            the run stops); chant keeps that as a plan, named by its digest
//   Approve  approve-release, bound to the plan: an approval is for that
//            commit, tag and notes, so the next release asks again
//   Cut      `scripts/release bump --plan`: bump, tag, push the tag with main
//            (refused if it would now release anything else), then the
//            release goes on chant's release ledger
//   Ship     follow the release workflow the tag started until it ends
//
// `just release-op [patch|minor|major] [--app] [COMMIT]` runs it with those
// in RELEASE_ARGS (what `just release` takes); a run that stops at the gate
// exits 3. Approve with Arugula's workspace block (the phone too), or
// `chant approve release approve-release --approver NAME`, then run it again
// with the same arguments: it walks through the gate and cuts.

// scripts/release, from the repository this op is in, whatever directory
// chant runs it from.
const release = `"$(git rev-parse --show-toplevel)/scripts/release"`;
const planFile = `"$(git rev-parse --show-toplevel)/dist/release/plan.json"`;

// RELEASE_ARGS split by the shell, as `just release` passes its own.
const pick = shell(`${release} plan \${RELEASE_ARGS:-} --out ${planFile}`, { id: "pick", timeout: "10m" });
const show = shell(`cat ${planFile}`, { id: "show", json: true });
const plan = releasePlan({
  id: "plan",
  component: "arugula",
  env: "github",
  gitSha: pick.out.stdout,
  content: show.out.json,
});

export default Op({
  name: "release",
  overview: "Release Arugula from the newest green commit on main, once Jake approves the plan",
  phases: [
    phase("Prep", [pick, show, plan]),
    phase("Approve", [
      gate("approve-release", {
        plan: plan.out.digest,
        timeout: "72h",
        description: "Release the plan in ops/dist/plans: its commit, tag and notes",
      }),
    ]),
    phase("Cut", [
      shell(`${release} bump --plan "$PLAN"`, { env: { PLAN: plan.out.file }, timeout: "30m" }),
      releaseRecord({ plan: plan.out.file, digest: plan.out.digest, approval: { op: "release", gate: "approve-release" } }),
    ]),
    phase("Ship", [
      shell(`${release} watch --plan "$PLAN"`, { env: { PLAN: plan.out.file }, timeout: "4h" }),
    ]),
  ],
});
