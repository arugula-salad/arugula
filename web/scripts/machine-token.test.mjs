// Machine tokens (aru-141): what a browser with no device key asks control
// for, how it says which machines it reaches, and when it uses tokens. The
// channel itself (the hello in message 1, a refused token replaced) is
// checked against Rust in e2e-interop.ts. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { controlTokens, NoTokenError, tokenEntry, tokenHello, tokenMode } from "../src/shell/e2e/machine-token.ts";

const K = (c) => c.repeat(64);

test("the hello is control-wire's TokenHello and nothing else", () => {
  assert.equal(new TextDecoder().decode(tokenHello("a.b.c")), '{"machine_token":"a.b.c"}');
});

test("a token is POSTed for, per machine, and a refusal says why", async () => {
  const sent = [];
  const send = async (url, init) => {
    sent.push({ url, method: init.method, body: init.body });
    if (url.includes("gone")) return new Response(JSON.stringify({ error: "not your machine" }), { status: 403 });
    return new Response(JSON.stringify({ token: `jwt-${sent.length}`, expires: 1, role: "owner" }), { status: 200 });
  };
  const t = controlTokens("", send);
  assert.equal(await t.token("d1"), "jwt-1");
  assert.equal(await t.token("d1"), "jwt-2", "a fresh one each time");
  assert.deepEqual(sent[0], { url: "/api/machines/d1/token", method: "POST", body: "{}" });
  await assert.rejects(t.token("gone"), (e) => e instanceof NoTokenError && e.noToken && e.status === 403 && /not your machine/.test(e.message));
});

test("tokens first, unless this browser says", () => {
  assert.equal(tokenMode(() => null), "first");
  assert.equal(tokenMode(() => "no-key"), "no-key");
  assert.equal(tokenMode(() => "first"), "first");
  assert.equal(tokenMode(() => "off"), "off");
  assert.equal(tokenMode(() => "nonsense"), "first");
  assert.equal(
    tokenMode(() => {
      throw new Error("no storage");
    }),
    "first",
  );
});

const base = { name: "box", urls: ["https://box.ts.net"], online: true, last_seen: null };
const daemonCert = (device, noise) => ({ v: 1, account: "a1", device, kind: "daemon", name: "box", noise, sign: K("b"), created: 5, approver: "r", sig: "s" });

test("a machine's noise key: the directory's, else a certificate beside it", () => {
  const given = tokenEntry({ ...base, id: "d1", noise: K("A") }, [], "a1", []);
  assert.equal(given.cert.noise, K("a"));
  assert.equal(given.cert.device, "d1");
  assert.deepEqual(given.urls, base.urls);
  // A control from before #994: the account's own certificates.
  assert.equal(tokenEntry({ ...base, id: "d2" }, [daemonCert("d2", K("c"))], "a1", []).cert.noise, K("c"));
  // Someone else's machine: the chain listed with it.
  const theirs = tokenEntry({ ...base, id: "d3", account: "a2", chain: { trust: null, certs: [daemonCert("d3", K("d"))], revocations: [] } }, [], "a1", []);
  assert.equal(theirs.cert.noise, K("d"));
  assert.equal(theirs.chain, undefined);
  // A browser's certificate is no machine's; nothing found leaves it out.
  assert.equal(tokenEntry({ ...base, id: "d4" }, [{ ...daemonCert("d4", K("e")), kind: "browser" }], "a1", []), null);
  assert.equal(tokenEntry({ ...base, id: "d5", noise: "not hex" }, [], "a1", []), null);
});

test("a hosted machine: its own certificate's key, the relay only, and its project", () => {
  const proof = (owner) => ({
    cert: { project: "prj_1", owner, workspace: "hw_1", delegation: "x", device: "h1", name: "hosted", noise: K("F"), sign: K("9"), created: 7, sig: "" },
    delegation: { project: "prj_1", owner, key: K("1"), at: 1, by: "r", sig: "" },
    revocations: [],
  });
  const mine = tokenEntry({ ...base, id: "h1", hosted: proof("account:a1") }, [], "a1", []);
  assert.deepEqual(mine.urls, []);
  assert.equal(mine.cert.noise, K("f"));
  assert.deepEqual(mine.hosted, { project: "prj_1", workspace: "hw_1", role: "owner" });
  const teams = [{ team: "t1", roster: { members: [{ account: "a1", role: "editor" }] } }];
  assert.equal(tokenEntry({ ...base, id: "h1", hosted: proof("team:t1") }, [], "a1", teams).hosted.role, "editor");
  assert.equal(tokenEntry({ ...base, id: "h1", hosted: proof("team:t2") }, [], "a1", teams).hosted.role, "viewer");
});

test("a device falls back to its key only when tokens can't reach the machine at all", async () => {
  const { keyAfterTokens } = await import("../src/shell/e2e/channel.ts");
  // A control with no issuer key (dev, tests) answers 404.
  assert.equal(keyAfterTokens(new NoTokenError(404, "no issuer")), true);
  // A daemon from before tokens reads the hello as a malformed block grant.
  assert.equal(keyAfterTokens(Object.assign(new Error("old"), { byDaemon: true, code: "malformed" })), true);
  // Signed out, not let in, or a token refused: the key wouldn't help.
  assert.equal(keyAfterTokens(new NoTokenError(401, "signed out")), false);
  assert.equal(keyAfterTokens(new NoTokenError(403, "not yours")), false);
  assert.equal(keyAfterTokens(Object.assign(new Error("used"), { byDaemon: true, code: "machine-token" })), false);
  assert.equal(keyAfterTokens(new Error("socket closed")), false);
});
