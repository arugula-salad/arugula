// The standing project grant (M211): the page's copy of
// `arugula_e2e::project_grant` writes the same bytes as Rust's
// (run-grant.fixture.json, which `the_pages_fixture_is_these_bytes` in
// crates/e2e/src/project_grant.rs pins): v1 grants and device revokes as
// the platform still holds them, and DK6's v2 grant and v3 revoke with the
// challenge a passkey signs. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { verify } from "../src/shell/e2e/cert.ts";
import { delegationId } from "../src/shell/e2e/hosted.ts";
import { grantBody, grantRevokeBody, newGrant, newGrantRevoke, passkeyChallenge } from "../src/shell/e2e/run-grant.ts";

const f = JSON.parse(readFileSync(new URL("./run-grant.fixture.json", import.meta.url), "utf8"));
const b64u = (b) => Buffer.from(b).toString("base64url");

test("the v1 bodies are Rust's", async () => {
  assert.equal(grantBody(f.grant), f.body);
  assert.equal(grantRevokeBody(f.revoke), f.revoke_body);
  assert.equal(f.grant.delegation, await delegationId(f.delegation));
  assert.ok(await verify(f.root.sign, f.body, f.grant.sig));
  assert.ok(await verify(f.root.sign, grantBody(f.expiring), f.expiring.sig));
  assert.ok(await verify(f.root.sign, f.revoke_body, f.revoke.sig));
});

test("a v2 grant and a v3 revoke made here are Rust's bytes, and their passkey challenges", async () => {
  assert.equal(grantBody(f.grant_v2), f.grant_v2_body);
  assert.equal(grantRevokeBody(f.revoke_v3), f.revoke_v3_body);
  const g = await newGrant(f.delegation, "acct1", f.grant_v2.expires, f.grant_v2.id, f.grant_v2.made);
  assert.deepEqual(g, f.grant_v2);
  assert.equal(grantBody(g), f.grant_v2_body);
  assert.equal(b64u(await passkeyChallenge(grantBody(g))), f.grant_v2_challenge);
  const r = newGrantRevoke(f.revoke_v3.project, f.revoke_v3.delegation, "stop", "acct1", f.revoke_v3.at);
  assert.deepEqual(r, f.revoke_v3);
  assert.equal(b64u(await passkeyChallenge(grantRevokeBody(r))), f.revoke_v3_challenge);
  // Any change is another challenge.
  assert.notEqual(b64u(await passkeyChallenge(grantBody({ ...g, caps: ["runs.start"] }))), f.grant_v2_challenge);
  assert.notEqual(b64u(await passkeyChallenge(grantRevokeBody({ ...r, running: "finish" }))), f.revoke_v3_challenge);
});

// Who could sign a v1 (control.ts's canApprove, which device approvals
// still use): a browser's own all the way back to the root, as control
// counts it (`Trust::chained_below`). DK8 (#389) retires it for grants.
test("a browser chained through a revoked browser and a grandfathered app window may sign", async () => {
  const { browserOrLegacyChained } = await import("../src/shell/e2e/cert.ts");
  const cert = (device, kind, approver, name = device) => ({ account: "a", device, kind, approver, name });
  const root = cert("root", "browser", "root", "Firefox on Linux");
  const window = cert("win", "browser", "root", "illogical app on geek");
  const revoked = cert("ffl", "browser", "win", "Firefox on Linux");
  const mac = cert("ffm", "browser", "ffl", "Firefox on Mac");
  const certs = [root, window, revoked, mac];
  const chained = (certs, me, root, legacy) => browserOrLegacyChained({ account: "a", root }, certs, me.device, legacy);
  // Jake's chain (#1394): the window counts just above what it approved
  // back then, and the revoked browser still links what it approved.
  assert.equal(chained(certs, mac, "root", new Set(["ffl"])), true);
  // Not grandfathered: the window isn't a browser.
  assert.equal(chained(certs, mac, "root", new Set()), false);
  // A link missing altogether, or another root, isn't a chain.
  assert.equal(chained([root, window, mac], mac, "root", new Set(["ffl"])), false);
  assert.equal(chained(certs, mac, "other", new Set(["ffl"])), false);
  // The window itself never signs, nor does the app or a CLI.
  assert.equal(chained(certs, window, "root", new Set(["ffl"])), false);
  const app = cert("app", "app", "ffm");
  assert.equal(chained([...certs, app], app, "root", new Set(["ffl"])), false);
  // Every certificate of a device on the way is a link: a second one made
  // by the app spoils it.
  const twice = cert("ffl", "browser", "app", "Firefox on Linux");
  assert.equal(chained([...certs, app, twice], mac, "root", new Set(["ffl"])), false);
  // A recovery code that let a browser in counts on the way back, never as
  // the signer.
  const code = cert("code", "recovery", "root");
  const phone = cert("ph", "browser", "code", "Safari on iPhone");
  assert.equal(chained([root, code, phone], phone, "root", new Set()), true);
  assert.equal(chained([root, code], code, "root", new Set()), false);
});
