// A project's environments and "Deploy…" on the platform page (M252): what
// the page sends to make an environment (the driver's settings, the
// secrets it may hold) and to plan a deploy (a full commit of a repo).
// Secrets' values are never part of either. Run with `node
// --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { COMMIT, environmentBody, historyPath, holdable, lastMoved, planBody, repoFor, rowNote, secretNote, shortRef } from "../src/platform/deploys-text.ts";
import { REPO } from "../src/platform/paths.ts";

// As `GET /api/projects/{project}/drivers` answers (crates/platform's
// deploys/catalog.rs).
const fly = {
  name: "chant-fly",
  rolls_back: true,
  settings: [
    { name: "fly_app", required: true, default: null, choices: [], about: "The Fly app it deploys." },
    { name: "chant_env", required: true, default: null, choices: [], about: "The ledger's environment." },
    { name: "chant_dir", required: false, default: ".", choices: [], about: "The member." },
    { name: "chant_digest", required: false, default: "plan", choices: ["plan", "image"], about: "What the digest is." },
  ],
  secrets: [
    { name: "FLY_API_TOKEN", minted: null },
    { name: "GITHUB_TOKEN", minted: "unless_held" },
  ],
};
const apps = {
  name: "apps-platform",
  rolls_back: true,
  settings: [],
  secrets: [
    { name: "APPS_PLATFORM_TOKEN", minted: "unless_held" },
    { name: "SOURCE_TOKEN", minted: "always" },
  ],
};

test("an environment's body: its name, its driver's settings given, the secrets it may hold", () => {
  const b = environmentBody(" production ", fly, { fly_app: " acme-web ", chant_env: "github", chant_dir: "" }, ["FLY_API_TOKEN"]);
  assert.deepEqual(b, {
    body: { name: "production", driver: "chant-fly", secret_names: ["FLY_API_TOKEN"], settings: { fly_app: "acme-web", chant_env: "github" } },
  });
  // A required setting left out, a choice it doesn't take, a bad name, no driver.
  assert.match(environmentBody("production", fly, { fly_app: "a" }, []).error, /chant_env is needed/);
  assert.match(environmentBody("production", fly, { fly_app: "a", chant_env: "g", chant_digest: "x" }, []).error, /plan or image/);
  for (const bad of ["Production", "-x", "a_b", "", "x".repeat(41)]) assert.match(environmentBody(bad, fly, {}, []).error, /a-z, 0-9/, bad);
  assert.equal(environmentBody("p", undefined, {}, []).error, "Pick a driver.");
  // A secret the platform always mints is never sent as one to hold.
  assert.deepEqual(environmentBody("apps", apps, {}, ["APPS_PLATFORM_TOKEN", "SOURCE_TOKEN"]).body.secret_names, ["APPS_PLATFORM_TOKEN"]);
  assert.deepEqual(
    holdable(apps).map((s) => s.name),
    ["APPS_PLATFORM_TOKEN"],
  );
  assert.equal(secretNote(apps.secrets[1]), "minted by the platform for each run");
  assert.match(secretNote(fly.secrets[1]), /unless the environment holds its own/);
  assert.equal(secretNote(fly.secrets[0]), "");
});

test("a plan's body: a full commit of a repo, to an environment", () => {
  const sha = "0123456789ABCDEF0123456789abcdef01234567";
  assert.deepEqual(planBody("env_1", ` ${sha} `, " acme/app ", true, REPO), {
    body: { environment: "env_1", source: { kind: "commit", ref: sha.toLowerCase(), where: "acme/app" }, roll_back: true },
  });
  assert.match(planBody("env_1", "deadbeef", "acme/app", false, REPO).error, /40 hex/);
  assert.match(planBody("env_1", sha, "acme", false, REPO).error, /OWNER\/NAME/);
  assert.equal(planBody("", sha, "acme/app", false, REPO).error, "Pick an environment.");
  assert.ok(COMMIT.test("a".repeat(40)) && !COMMIT.test("a".repeat(39)) && !COMMIT.test("g".repeat(40)));
});

test("an environment's history: its page of the project's deploys, and when each last moved", () => {
  assert.equal(historyPath("prj 1"), "/api/projects/prj%201/deploys");
  assert.equal(historyPath("prj_1", "env_1", 5), "/api/projects/prj_1/deploys?environment=env_1&limit=5");
  assert.equal(historyPath("prj_1", "env_1", 20, "dep_9"), "/api/projects/prj_1/deploys?environment=env_1&limit=20&before=dep_9");
  assert.equal(historyPath("prj_1", undefined, undefined, null), "/api/projects/prj_1/deploys");
  // Ended, then decided, then planned.
  assert.equal(lastMoved({ created_at: 1, decided_at: 2, finished_at: 3 }), 3);
  assert.equal(lastMoved({ created_at: 1, decided_at: 2, finished_at: null }), 2);
  assert.equal(lastMoved({ created_at: 1, decided_at: null, finished_at: null }), 1);
  assert.equal(shortRef("0123456789abcdef0123456789abcdef01234567"), "0123456789ab");
  // An artifact's digest: 12 hex after its `sha256:` (UI audit #167).
  assert.equal(shortRef(`sha256:c09af${"0".repeat(59)}`), "c09af0000000");
});

test("Deploy… offers the environment's repo: its last deploy's, else the one named as it is, else the first (UI audit #147)", () => {
  const linked = ["arugula-salad/arugula", "arugula-salad/beds-smoke"];
  assert.equal(repoFor("beds-smoke", linked, null), "arugula-salad/beds-smoke");
  assert.equal(repoFor("Beds-Smoke", linked), "arugula-salad/beds-smoke");
  assert.equal(repoFor("beds-smoke", linked, "acme/other"), "acme/other");
  assert.equal(repoFor("production", linked, null), "arugula-salad/arugula");
  assert.equal(repoFor(undefined, linked), "arugula-salad/arugula");
  assert.equal(repoFor("production", [], null), "");
});

// #333: a history row's note beside the ref; a release's version is always
// expected, and nothing read is nothing said.
test("a history row's note is the expected version and the subject", () => {
  const row = (driver, subject, expected_version) => ({ environment: { name: "release", driver }, subject, expected_version });
  assert.equal(rowNote(row("arugula-release", "Docs: the Arugula look", "0.63.10")), "0.63.10 (expected) · Docs: the Arugula look");
  assert.equal(rowNote(row("chant-fly", "Fix", "9.9.9")), "Fix");
  assert.equal(rowNote(row("chant-fly", null, null)), "");
});
