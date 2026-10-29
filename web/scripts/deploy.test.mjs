// A deploy's approval (M218): the page's copy of `arugula_e2e::deploy`
// reads and signs the same bytes as Rust's (deploy.fixture.json, which
// `the_pages_fixture_is_these_bytes` in crates/e2e/src/deploy.rs pins); a
// push for one opens its card on the platform page, at `/deploys/:id`.
// Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { actorOf, approvalBody, checkPlan, eventCanonical, parseOperation, sha256Hex, signApproval } from "../src/shell/e2e/deploy.ts";
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

test("an approval signs Rust's bytes, with Rust's signature", async () => {
  const { sig, ...a } = f.approval;
  assert.equal(approvalBody(a), f.approval_body);
  // PKCS#8 for an Ed25519 seed (RFC 8410), as the page imports one.
  const pkcs8 = new Uint8Array([...hexOf("302e020100300506032b657004220420"), ...hexOf(f.seed)]);
  const key = await crypto.subtle.importKey("pkcs8", pkcs8, { name: "Ed25519" }, false, ["sign"]);
  assert.equal((await signApproval(a, key)).sig, sig);
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
