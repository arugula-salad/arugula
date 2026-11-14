// The app's session (Arugula #989): the machines it lists from the hub's
// directory, how it reaches them (the relay on its own origin, a machine
// token, no device key), the account's prefs, and today's links while the
// hub can't reach machines from the app's host. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { AppSession } from "../src/app/session.ts";
import { NoTokenError } from "../src/shell/e2e/machine-token.ts";

const K = (c) => c.repeat(64);
const ORIGIN = "https://app.arugula.io";
const CONTROL = "https://control.arugula.io";
const me = () => ({ account: "acc1", login: "sam" });

/** A hub answering `routes` (path → [status, body]), and what was sent. */
function hub(routes) {
  const sent = [];
  const send = async (url, init = {}) => {
    sent.push({ url, method: init.method ?? "GET", body: init.body, credentials: init.credentials });
    const r = routes[url];
    if (!r) return new Response("{}", { status: 404 });
    if (r instanceof Error) throw r;
    const [status, body] = typeof r === "function" ? r() : r;
    return new Response(JSON.stringify(body), { status });
  };
  return { send, sent };
}

const DIR = {
  daemons: [
    { id: "d1", name: "mini", urls: ["https://mini.tail:7681"], online: true, last_seen: 1, noise: K("a") },
    { id: "d2", name: "laptop", urls: [], online: false, last_seen: 2, noise: K("b"), account: "acc2", owner_name: "jo", team: "t1" },
    // A control from before #994 gives no key: not reachable with a token.
    { id: "d3", name: "old", urls: [], online: true, last_seen: 3 },
  ],
};

test("the directory lists the machines with a Noise key, and the session is ready and reachable", async () => {
  const h = hub({ "/api/directory": [200, DIR] });
  const s = new AppSession(me, CONTROL, ORIGIN, h.send);
  let heard = 0;
  s.subscribe(() => heard++);
  assert.equal(s.phase, "loading");
  assert.equal(s.reachable, null);
  await s.refresh();
  assert.deepEqual(
    s.daemons.map((d) => [d.id, d.cert.noise, d.owner_name ?? null]),
    [
      ["d1", K("a"), null],
      ["d2", K("b"), "jo"],
    ],
  );
  assert.equal(s.phase, "ready");
  assert.equal(s.reachable, true);
  assert.equal(s.stale, false);
  assert.equal(s.login, "sam");
  assert.equal(heard, 1);
  assert.deepEqual(h.sent[0], { url: "/api/directory", method: "GET", body: undefined, credentials: "same-origin" });
});

test("a machine is reached through the relay on the app's own origin, with tokens and no device key", async () => {
  const h = hub({ "/api/directory": [200, DIR], "/api/machines/d1/token": [200, { token: "jwt-1", expires: 1, role: "owner" }] });
  const s = new AppSession(me, CONTROL, ORIGIN, h.send);
  await s.refresh();
  const t = s.target("d1");
  assert.deepEqual(t.daemon, { id: "d1", noise: K("a") });
  assert.equal(t.relay, "wss://app.arugula.io/api/relay/c/d1");
  assert.equal(t.mux, "wss://app.arugula.io/api/relay/m");
  // The page's CSP lets it reach its own origin only: no machine's address.
  assert.deepEqual(t.direct, []);
  assert.equal(t.keys, undefined);
  assert.equal(await t.tokens.token("d1"), "jwt-1");
  assert.deepEqual(h.sent.at(-1), { url: "/api/machines/d1/token", method: "POST", body: "{}", credentials: "same-origin" });
  assert.equal(s.target("nope"), undefined);
  // A dev page on http gets ws.
  const local = new AppSession(me, CONTROL, "http://app.localhost:7690", h.send);
  await local.refresh();
  assert.equal(local.target("d1").relay, "ws://app.localhost:7690/api/relay/c/d1");
});

test("before the cloud lets the directory through on the app's host, it's unreachable: today's links", async () => {
  const s = new AppSession(me, CONTROL, ORIGIN, hub({}).send);
  let heard = 0;
  s.subscribe(() => heard++);
  await s.refresh();
  assert.equal(s.reachable, false);
  assert.equal(s.phase, "ready");
  assert.deepEqual(s.daemons, []);
  assert.equal(heard, 1);
  assert.equal(s.fallback("pane=d1.4"), "https://control.arugula.io/#pane=d1.4");
  assert.equal(s.fallback("swarm"), "https://control.arugula.io/#swarm");
});

test("a token route that isn't there makes it unreachable, once; another refusal doesn't", async () => {
  let tokenAnswer = [403, { error: "not your machine" }];
  const h = hub({ "/api/directory": [200, DIR], "/api/machines/d1/token": () => tokenAnswer });
  const s = new AppSession(me, CONTROL, ORIGIN, h.send);
  await s.refresh();
  let heard = 0;
  s.subscribe(() => heard++);
  const t = s.target("d1");
  await assert.rejects(t.tokens.token("d1"), (e) => e instanceof NoTokenError && e.status === 403);
  assert.equal(s.reachable, true);
  // A refusal holds that machine's asks back a minute (the 429 outage).
  await assert.rejects(t.tokens.token("d1"), (e) => e instanceof NoTokenError && e.retryMs > 0);
  tokenAnswer = [404, {}];
  const s2 = new AppSession(me, CONTROL, ORIGIN, h.send);
  await s2.refresh();
  s2.subscribe(() => heard++);
  const t2 = s2.target("d1");
  await assert.rejects(t2.tokens.token("d1"), (e) => e instanceof NoTokenError && e.status === 404);
  await assert.rejects(t2.tokens.token("d1"), (e) => e instanceof NoTokenError);
  assert.equal(s2.reachable, false);
  assert.equal(heard, 1);
});

test("signed out, the machines go; a failed read keeps them, offline and stale", async () => {
  let answer = [200, DIR];
  const s = new AppSession(me, CONTROL, ORIGIN, hub({ "/api/directory": () => answer }).send);
  await s.refresh();
  answer = [500, { error: "down" }];
  await s.refresh();
  assert.equal(s.stale, true);
  assert.equal(s.phase, "ready");
  assert.deepEqual(
    s.daemons.map((d) => [d.id, d.online]),
    [
      ["d1", false],
      ["d2", false],
    ],
  );
  answer = [401, { error: "signed out" }];
  await s.refresh();
  assert.equal(s.phase, "signed-out");
  assert.deepEqual(s.daemons, []);
});

test("a first read that can't reach the hub is an error, not ready", async () => {
  const s = new AppSession(me, CONTROL, ORIGIN, hub({ "/api/directory": new TypeError("offline") }).send);
  await s.refresh();
  assert.equal(s.phase, "error");
  assert.equal(s.stale, true);
  assert.equal(s.reachable, null);
});

test("the account's prefs are read and set on /api/me/prefs", async () => {
  const h = hub({ "/api/me/prefs": [200, { "app.project": "prj_1" }] });
  const s = new AppSession(me, CONTROL, ORIGIN, h.send);
  assert.deepEqual(await s.prefs(), { "app.project": "prj_1" });
  await s.setPrefs({ "app.project": "prj_2", tips: null });
  assert.deepEqual(h.sent.at(-1), { url: "/api/me/prefs", method: "POST", body: '{"app.project":"prj_2","tips":null}', credentials: "same-origin" });
  await assert.rejects(new AppSession(me, CONTROL, ORIGIN, hub({}).send).prefs(), /HTTP 404/);
});

test("the hub's block domain reaches every target; an older hub's 404, or a value that isn't a host, leaves none", async () => {
  const at = async (config) => {
    const h = hub({ "/api/directory": [200, DIR], ...(config ? { "/api/app/config": config } : {}) });
    const s = new AppSession(me, CONTROL, ORIGIN, h.send);
    await s.config();
    await s.refresh();
    return s;
  };
  const on = await at([200, { block_domain: "arugula.dev" }]);
  assert.equal(on.blockDomain, "arugula.dev");
  assert.equal(on.target("d1").blockDomain, "arugula.dev");
  assert.equal(on.target("d2").blockDomain, "arugula.dev");
  // A dev hub's, with its port.
  assert.equal((await at([200, { block_domain: "block.localhost:7690" }])).target("d1").blockDomain, "block.localhost:7690");
  for (const config of [null, [404, {}], [200, { block_domain: null }], [200, {}], [200, { block_domain: "https://evil.example/x" }], [500, { error: "down" }]]) {
    const s = await at(config);
    assert.equal(s.blockDomain, null, JSON.stringify(config));
    assert.equal("blockDomain" in s.target("d1"), false, JSON.stringify(config));
  }
});

test("start reads the config before the machines, so the first targets have it", async () => {
  const h = hub({ "/api/app/config": [200, { block_domain: "arugula.dev" }], "/api/directory": [200, DIR] });
  const s = new AppSession(me, CONTROL, ORIGIN, h.send);
  const ready = new Promise((done) => s.subscribe(done));
  const stop = s.start();
  await ready;
  stop();
  assert.deepEqual(
    h.sent.map((x) => x.url),
    ["/api/app/config", "/api/directory"],
  );
  assert.equal(s.target("d1").blockDomain, "arugula.dev");
});
