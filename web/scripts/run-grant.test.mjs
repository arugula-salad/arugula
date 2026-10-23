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

// Who may sign one (control.ts's canApprove): a browser's own all the way
// back to the root, as control counts it (`Trust::chained_below`).
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
