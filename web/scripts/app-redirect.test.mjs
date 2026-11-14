// Control's pages move to the app (Arugula #989): only once the cloud says
// so (`pages: "app"`), only its page addresses, and a bare one only for
// someone signed in. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { appAddress, appOrigin } from "../src/shell/app-redirect.ts";

const APP = "https://app.arugula.io";
const on = { control: true, pages: "app", platform: APP };
const D = "da5b786733ac68a9";

test("nothing moves until the cloud says so, or without the app's origin", () => {
  for (const info of [null, { control: true, platform: APP }, { pages: "control", platform: APP }, { pages: "app" }, { pages: "app", platform: null }, { pages: "app", platform: "javascript:alert(1)" }, { pages: "app", platform: "https://app.arugula.io/x" }]) {
    assert.equal(appOrigin(info), null, JSON.stringify(info));
    assert.equal(appAddress(info, `#pane=${D}.3`, "", true), null, JSON.stringify(info));
  }
  assert.equal(appOrigin(on), APP);
});

test("a page address opens at its place in the app, signed in or not", () => {
  for (const signedIn of [true, false]) {
    assert.equal(appAddress(on, `#pane=${D}.3&share`, "", signedIn), `${APP}/machines/${D}?pane=3&share`);
    assert.equal(appAddress(on, "#swarm", "", signedIn), `${APP}/machines/swarm`);
    assert.equal(appAddress(on, `#machine=${D}&agent`, "", signedIn), `${APP}/machines/${D}?agent`);
    assert.equal(appAddress(on, "", "?machine=mini", signedIn), `${APP}/machines?machine=mini`);
    assert.equal(appAddress(on, "#settings=machine", "", signedIn), `${APP}/machines#settings=machine`);
  }
});

test("bare, it moves only for someone signed in; signing in, secrets and the rest stay", () => {
  assert.equal(appAddress(on, "", "", true), `${APP}/`);
  assert.equal(appAddress(on, "", "", false), null);
  for (const h of ["#platform-signin=x", "#issuer-signin=x", "#link=a.b.c.d", "#join=x", "#device=x", "#k=x", "#add-passkey", "#reset", "#email=verified", "#github-link=ok", "#deploy=dep_0", "#chat=ch_0123456789abcdef", "#settings=account", "#settings", "#getting-started=cloud", "#teams"])
    assert.equal(appAddress(on, h, "", true), null, h);
});
