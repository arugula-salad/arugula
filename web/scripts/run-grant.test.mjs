// The standing project grant (M211): the page's copy of
// `arugula_e2e::project_grant` writes the same bytes as Rust's
// (run-grant.fixture.json, which `the_pages_fixture_is_these_bytes` in
// crates/e2e/src/project_grant.rs pins), and what it signs checks out
// against the same body. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { verify } from "../src/shell/e2e/cert.ts";
import { delegationId } from "../src/shell/e2e/hosted.ts";
import { generateKeys } from "../src/shell/e2e/keys.ts";
import { grantBody, grantRevokeBody, signGrant, signGrantRevoke } from "../src/shell/e2e/run-grant.ts";

const f = JSON.parse(readFileSync(new URL("./run-grant.fixture.json", import.meta.url), "utf8"));

test("the bodies are Rust's", async () => {
  assert.equal(grantBody(f.grant), f.body);
  assert.equal(grantRevokeBody(f.revoke), f.revoke_body);
  assert.equal(f.grant.delegation, await delegationId(f.delegation));
  assert.ok(await verify(f.root.sign, f.body, f.grant.sig));
  assert.ok(await verify(f.root.sign, grantBody(f.expiring), f.expiring.sig));
  assert.ok(await verify(f.root.sign, f.revoke_body, f.revoke.sig));
});

test("a grant signed here is the one Rust would sign, and checks out", async () => {
  const keys = await generateKeys();
  const g = await signGrant(f.delegation, keys, null, f.grant.id, f.grant.made);
  assert.equal(grantBody(g), f.body.replace(`by ${f.grant.by}`, `by ${keys.id}`));
  assert.ok(await verify(keys.signPub, grantBody(g), g.sig));
  // Changed after signing: the signature no longer checks out.
  assert.ok(!(await verify(keys.signPub, grantBody({ ...g, caps: ["runs.start"] }), g.sig)));
  const r = await signGrantRevoke(g.project, g.delegation, "finish", keys, 4000);
  assert.ok(await verify(keys.signPub, grantRevokeBody(r), r.sig));
  assert.ok(!(await verify(keys.signPub, grantRevokeBody({ ...r, running: "stop" }), r.sig)));
  assert.ok(!(await verify(keys.signPub, grantRevokeBody({ ...r, delegation: "other" }), r.sig)));
});
