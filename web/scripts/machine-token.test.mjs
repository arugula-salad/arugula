// Machine tokens (aru-141): what a browser with no device key asks control
// for, how it says which machines it reaches, and when it uses tokens. The
// channel itself (the hello in message 1, a refused token replaced) is
// checked against Rust in e2e-interop.ts. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { controlTokens, gatedTokens, NoTokenError, tokenEntry, tokenHello, tokenMode } from "../src/shell/e2e/machine-token.ts";

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

// The 2026-11-14 outage: a page retrying offline machines every ≤5 s asked
// for a token on every try (12 a minute per offline machine, 720 an hour:
// five of them on one page, or two on a few tabs, ran past the hub's 3,000).
// A page's reconnect loop, simulated: each machine tries, and tries again
// after the client's backoff (250 ms doubling to 5 s) or what the gate says.
async function simulate({ machines, offline, minutes, status = () => 200, retryAfter }) {
  let clock = 0;
  let calls = 0;
  const send = async (url) => {
    calls++;
    const s = status(clock);
    if (s === 200) return new Response(JSON.stringify({ token: `jwt-${calls}` }), { status: 200 });
    return new Response("{}", { status: s, headers: retryAfter ? { "Retry-After": String(retryAfter) } : {} });
  };
  const online = (id) => Number(id.slice(1)) >= offline;
  const t = gatedTokens(controlTokens("", send), online, () => clock);
  const next = new Map();
  const retry = new Map();
  for (let i = 0; i < machines; i++) next.set(`m${i}`, 0);
  const end = minutes * 60_000;
  while (true) {
    const [id, at] = [...next].reduce((a, b) => (b[1] < a[1] ? b : a));
    if (at > end) break;
    clock = at;
    const r = retry.get(id) ?? 0;
    try {
      await t.token(id);
      // An online machine connects and stays: no more tries.
      next.set(id, Infinity);
    } catch (e) {
      next.set(id, clock + (e.retryMs ?? Math.min(250 * 2 ** r, 5000)));
      retry.set(id, r + 1);
    }
  }
  return calls;
}

test("20 machines, 15 offline, 10 minutes: 5 token asks (was ~1,800, i.e. ~10,800/hour)", async () => {
  const calls = await simulate({ machines: 20, offline: 15, minutes: 10 });
  assert.equal(calls, 5, "one per online machine; none for offline ones");
});

test("a 429 holds every machine on the page back: well under 3,000/hour", async () => {
  // The hub answers 429 throughout: 20 machines, all online, 10 minutes.
  const calls = await simulate({ machines: 20, offline: 0, minutes: 10, status: () => 429 });
  // 1 + backoffs of 1, 2, 4 min fit in 10: ~4 asks, ~24/hour.
  assert.ok(calls <= 5, `${calls} asks`);
});

test("a 429 honours Retry-After when it's longer, and says so plainly", async () => {
  let clock = 0;
  let calls = 0;
  const send = async () => (calls++, new Response("{}", { status: 429, headers: { "Retry-After": "300" } }));
  const t = gatedTokens(controlTokens("", send), () => true, () => clock);
  await assert.rejects(t.token("a"), (e) => e.status === 429 && e.retryMs === 300_000 && /limiting connections; trying again in 300 s/.test(e.message));
  clock = 299_000;
  await assert.rejects(t.token("b"), (e) => e.retryMs === 1000);
  assert.equal(calls, 1, "another machine waits too");
  clock = 300_000;
  await assert.rejects(t.token("b"));
  assert.equal(calls, 2);
});

test("one ask per machine at a time; a 404 isn't held", async () => {
  let open = 0;
  let most = 0;
  const send = async (url) => {
    if (url.includes("none")) return new Response("{}", { status: 404 });
    most = Math.max(most, ++open);
    await new Promise((r) => setTimeout(r, 5));
    open--;
    return new Response(JSON.stringify({ token: "t" }), { status: 200 });
  };
  const t = gatedTokens(controlTokens("", send));
  await Promise.all([t.token("a"), t.token("a"), t.token("a")]);
  assert.equal(most, 1);
  await assert.rejects(t.token("none"), (e) => e.status === 404 && e.retryMs === undefined);
  await assert.rejects(t.token("none"), (e) => e.status === 404);
});

test("reconnects to an offline machine reuse one token until it's sent (keymaster's fix)", async () => {
  const { E2ESocket } = await import("../src/shell/e2e/channel.ts");
  let asked = 0;
  const tokens = { token: async () => `jwt-${++asked}` };
  // Nothing listens here: message 1 never goes, as to an offline machine.
  const urls = [{ url: "ws://127.0.0.1:9/e2e", timeoutMs: 1000 }];
  const daemon = { id: "offline-1", noise: "a".repeat(64) };
  for (let i = 0; i < 5; i++) await assert.rejects(E2ESocket.connect(urls, daemon, tokens));
  assert.equal(asked, 1, "5 reconnects, 1 token (was 5: 720/hour per offline machine per page at the 5 s retry)");
});

test("a kept token is taken once, and not within 30 s of expiring", async () => {
  const { keepUnspent, takeUnspent } = await import("../src/shell/e2e/channel.ts");
  const jwt = (exp) => `h.${Buffer.from(JSON.stringify({ exp })).toString("base64url")}.s`;
  keepUnspent("m", jwt(1000), 0);
  assert.equal(takeUnspent("m", 969_000), jwt(1000));
  assert.equal(takeUnspent("m", 0), null, "taken: no other try sends it too");
  keepUnspent("m", jwt(1000), 0);
  assert.equal(takeUnspent("m", 970_000), null, "too close to its expiry");
  keepUnspent("m", "opaque", 0);
  assert.equal(takeUnspent("m", 59_000), "opaque");
  keepUnspent("m", "opaque", 0);
  assert.equal(takeUnspent("m", 60_000), null, "one with no exp is kept a minute");
});
