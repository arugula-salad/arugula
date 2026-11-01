// A deploy's approval (M218, DK4): the page's copy of `arugula_e2e::deploy`
// reads and makes the same bytes as Rust's (deploy.fixture.json, which
// `the_pages_fixture_is_these_bytes` in crates/e2e/src/deploy.rs pins); a
// push for one opens its card on the platform page, at `/deploys/:id`.
// Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  NotHere,
  actorOf,
  approvalBody,
  approvalChallenge,
  checkDeploy,
  checkPlan,
  eventCanonical,
  parseOperation,
  refusedHow,
  sameSite,
  sha256Hex,
} from "../src/shared/deploy-approval.ts";
import { platformTap } from "../src/shell/tap.ts";
import { parse, pathOf } from "../src/platform/paths.ts";
import { deployTitle, staleNote, waitedText } from "../src/shared/deploys.ts";

const f = JSON.parse(readFileSync(new URL("./deploy.fixture.json", import.meta.url), "utf8"));
const hexOf = (s) => Uint8Array.from(s.match(/../g).map((b) => parseInt(b, 16)));

test("the operation's bytes are Rust's, read and hashed the same", async () => {
  assert.equal(await sha256Hex(f.operation.body), f.operation.hash);
  assert.deepEqual(parseOperation(f.operation.body), {
    project: "prj_1",
    environment: "env_1",
    name: "production",
    plan: "ab".repeat(32),
    source: { kind: "commit", ref: "deadbeef", where: "acme/app" },
    planned: "ef".repeat(32),
  });
  // Anything else isn't an operation: an extra line, a space in a value.
  assert.equal(parseOperation(f.operation.body + "x\n"), null);
  assert.equal(parseOperation(f.operation.body.replace("acme/app", "acme app")), null);
  assert.equal(parseOperation(f.operation.body.replace("name production", "name Production!")), null);
});

test("a planned event hashes as M217's does, and says who planned it", async () => {
  // M217's known answer (an_events_canonical_form_is_pinned).
  const e = {
    environment: "env_1",
    seq: 1,
    deploy: "dep_1",
    kind: "planned",
    actor: "agent acct geek",
    at: 1_700_000_000_000,
    detail: '{"plan":"ff"}',
    prev_hash: "0".repeat(64),
    hash: "",
  };
  assert.ok(eventCanonical(e).endsWith("actor agent acct geek\nat 1700000000000\ndetail eyJwbGFuIjoiZmYifQ==\n"));
  assert.equal(await sha256Hex(eventCanonical(e)), "5771e7fb68963ef115a17ee48691dbf38fb4edaf93281e30093a90a936e1569d"); // gitleaks:allow (a test hash)
  assert.deepEqual(actorOf("agent acct geek"), { account: "acct", machine: "geek" });
  assert.deepEqual(actorOf("person acct"), { account: "acct" });
  assert.equal(actorOf("run run_1"), null);
});

test("a v2 approval is Rust's bytes, and its passkey's challenge their SHA-256", async () => {
  const { by, sig, ...a } = f.approval_v2;
  assert.equal(by, "");
  assert.equal(sig, "");
  assert.equal(approvalBody(a), f.approval_v2_body);
  const b64u = (b) => btoa(String.fromCharCode(...b)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  assert.equal(b64u(await approvalChallenge(a)), f.approval_v2_challenge);
});

test("a v1 approval (before DK4) still reads as Rust's bytes, and its signature checks", async () => {
  const { sig, ...a } = f.approval;
  assert.equal(approvalBody(a), f.approval_body);
  const key = await crypto.subtle.importKey("raw", hexOf(f.sign), { name: "Ed25519" }, false, ["verify"]);
  assert.ok(await crypto.subtle.verify("Ed25519", key, hexOf(sig), new TextEncoder().encode(f.approval_body)));
});

test("a deploy's push opens its card on the platform page", () => {
  const id = "dep_00112233445566778899aabbccddeeff";
  assert.equal(platformTap({ deploy: id }), `/deploys/${id}`);
  assert.equal(platformTap({ deploy: "dep_nope" }), undefined);
  assert.deepEqual(parse(`/deploys/${id}`), { page: "deploy", deploy: id });
  assert.equal(pathOf({ page: "deploy", deploy: id }), `/deploys/${id}`);
  assert.equal(parse("/deploys/dep_x").page, "missing");
});

// M220: the dialog shows the plan's text only when it hashes to the plan
// the operation names, and calls out a roll back and its image.
test("a plan's text is shown only when it's the plan signed", async () => {
  const text =
    "arugula deploy plan v1\ndriver chant-fly\nenvironment production\nsource commit " +
    "a".repeat(40) +
    "\nwhere acme/app\nsetting fly_app acme-web\nroll back to dep_" +
    "1".repeat(32) +
    "\nback image registry.fly.io/acme-web:deployment-1@sha256:" +
    "0".repeat(64) +
    "\ndoes: fly deploy\n";
  const digest = await sha256Hex(text);
  const ok = await checkPlan(text, digest);
  assert.equal(ok.driver, "chant-fly");
  assert.deepEqual(ok.rollBack, { deploy: "dep_" + "1".repeat(32), image: "registry.fly.io/acme-web:deployment-1@sha256:" + "0".repeat(64) });
  const forward = text.replace(/roll back to .*\nback image .*\n/, "");
  assert.equal((await checkPlan(forward, await sha256Hex(forward))).rollBack, null);
  // Another app, the same digest: not the plan signed.
  assert.equal(await checkPlan(text.replace("acme-web\n", "acme-evil\n"), digest), "its plan's text isn't the plan you'd sign");
  assert.equal(await checkPlan(null, digest), "the platform didn't show its plan");
  assert.equal(await checkPlan("x\n", await sha256Hex("x\n")), "its plan's text can't be read");
});

// #333: the pages and the push say the same words (crates/platform/src/
// drivers/titles_tests.rs has the same cases), a release's version is
// always "expected", and without facts it's today's "Deploy to …".
test("a deploy is titled by what it deploys, and a release's version is expected", () => {
  const ref = "b1faaa04c0ffee00b1faaa04c0ffee00b1faaa04";
  const d = (driver, subject, expected_version) => ({ environment: { name: "production", driver }, source: { ref }, subject, expected_version });
  assert.equal(deployTitle(d("arugula-release", "Docs: the Arugula look", "0.63.9")), "Release 0.63.9 (expected) · b1faaa04 · Docs: the Arugula look");
  assert.equal(deployTitle(d("arugula-release", null, "0.63.9")), "Release 0.63.9 (expected) · b1faaa04");
  assert.equal(deployTitle(d("arugula-release", "Fix", null)), "Deploy b1faaa04 to production · Fix");
  assert.equal(deployTitle(d("chant-fly", "Fix", "9.9.9")), "Deploy b1faaa04 to production · Fix");
  assert.equal(deployTitle(d("chant-fly", null, null)), "Deploy to production");
});

test("how long a deploy has waited", () => {
  const now = 1_700_000_000_000;
  assert.deepEqual(
    [20_000, 12 * 60_000, 3 * 3_600_000 + 5 * 60_000, 2 * 86_400_000].map((ms) => waitedText(now - ms, now)),
    ["under a minute", "12m", "3h", "2d"],
  );
});

test("a stale deploy says why, by what overtook it (#334)", () => {
  assert.equal(
    staleNote({ why: "superseded" }, "release"),
    "A newer deploy to release was asked for after this, so it won't be applied.",
  );
  const applied = "Another deploy was applied to release after this was planned, so it can't be approved. Plan it again.";
  assert.equal(staleNote({ why: "applied" }, "release"), applied);
  // An old event has no why: it was an apply.
  assert.equal(staleNote({}, "release"), applied);
});

/** A deploy as the platform answers it, its bytes consistent: the plan's
 * text, its `planned` event and the operation over both. */
async function aDeploy() {
  const ref = "a".repeat(40);
  const plan_text = `arugula deploy plan v1\ndriver fake\nenvironment production\nsource commit ${ref}\nwhere acme/app\n`;
  const plan = await sha256Hex(plan_text);
  const e = {
    environment: "env_1",
    seq: 1,
    deploy: "dep_1",
    kind: "planned",
    actor: "person acc_a",
    at: 1_700_000_000_000,
    detail: JSON.stringify({ plan, source: { kind: "commit", ref, where: "acme/app" } }),
    prev_hash: "0".repeat(64),
    hash: "",
  };
  e.hash = await sha256Hex(eventCanonical(e));
  const body = `arugula deploy op v1\nproject prj_1\nenvironment env_1\nname production\nplan ${plan}\nsource commit ${ref}\nwhere acme/app\nplanned ${e.hash}\n`;
  return {
    id: "dep_1",
    project: { id: "prj_1", name: "Acme" },
    environment: { id: "env_1", name: "production", driver: "fake" },
    source: { kind: "commit", ref, where: "acme/app" },
    plan,
    plan_text,
    planned_by: "acc_a",
    planned_by_name: "Ada",
    agent: null,
    operation: { body, hash: await sha256Hex(body) },
    planned_event: e,
  };
}

// DK5: the platform page checks what it signs as control's dialog does, and
// offers Approve only over bytes that check out.
test("a deploy is offered for approval only when its bytes check out", async () => {
  const ok = await checkDeploy(await aDeploy());
  assert.equal(typeof ok, "object", String(ok));
  assert.equal(ok.op.name, "production");
  assert.equal(ok.planner, "Ada");
  // Bytes that aren't their hash, an operation that names another place, a
  // planned event that isn't the one named, someone else as the planner.
  const d = await aDeploy();
  d.operation.hash = "0".repeat(64);
  assert.equal(await checkDeploy(d), "its operation's bytes don't match their hash");
  const other = await aDeploy();
  other.environment.name = "staging";
  assert.equal(await checkDeploy(other), "its operation says something other than the deploy");
  const event = await aDeploy();
  event.planned_event.at += 1;
  assert.equal(await checkDeploy(event), "its plan's event doesn't match the operation");
  const who = await aDeploy();
  who.planned_by = "acc_b";
  assert.equal(await checkDeploy(who), "who planned it doesn't match its log");
  const none = await aDeploy();
  none.planned_event = null;
  assert.equal(await checkDeploy(none), "its plan isn't in the log");
});

// DK5 (S46 q1): where control's passkeys can't be used on this page, the
// page offers control's dialog instead; a person turning the prompt down is
// only that.
test("a refused passkey prompt says whether to go round by Control", () => {
  assert.ok(refusedHow("SecurityError", 4000, false) instanceof NotHere);
  assert.ok(refusedHow("NotAllowedError", 200, false) instanceof NotHere, "too fast for a person: a manager without related origins");
  const person = refusedHow("NotAllowedError", 8000, false);
  assert.ok(person && !(person instanceof NotHere), "a person said no");
  assert.ok(!(refusedHow("NotAllowedError", 200, true) instanceof NotHere), "on control's own site there's no way round");
  assert.equal(refusedHow("AbortError", 10, false), null);
  // Control's host, or a parent of this page's host: no related origins needed.
  assert.ok(sameSite("control.arugula.io", "control.arugula.io"));
  assert.ok(sameSite("localhost", "app.localhost"));
  assert.ok(!sameSite("control.arugula.io", "app.arugula.io"));
  assert.ok(!sameSite("arugula.io", "evilarugula.io"));
});
