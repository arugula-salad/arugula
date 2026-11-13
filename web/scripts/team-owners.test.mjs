// Owner changes (DK7): the page's `changesOwners` and `ownerChallenge` run
// Rust's example file (contract/examples/logic/team/owners.json, written by
// crates/e2e/tests/logic/team.rs) case by case and give the same output:
// which versions change who the owners are, and what an owner's passkey
// signs for one. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { changesOwners, ownerChallenge } from "../src/shell/e2e/team.ts";

const f = JSON.parse(readFileSync(new URL("../../contract/examples/logic/team/owners.json", import.meta.url), "utf8"));
const b64u = (b) => Buffer.from(b).toString("base64url");

test("which versions change the owners, and their challenge, are Rust's", async () => {
  assert.ok(f.cases.length > 0);
  for (const c of f.cases) {
    const prev = f.objects[c.input.prev];
    const r = f.objects[c.input.roster];
    const got = { changes_owners: changesOwners(r, prev), challenge: b64u(await ownerChallenge(r)) };
    assert.deepEqual(got, c.output, c.about);
  }
});
