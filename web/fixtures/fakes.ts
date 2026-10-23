// The outside services arugula-control talks to, faked: GitHub sign-in,
// Stripe and a Web Push endpoint. `web/control-smoke.ts` starts them in its
// own process; the test stack's `control` profile (testnet/) runs them in a
// container with `fakes-serve.ts`. Each records what it was sent, for the
// test to check.

import { createServer, type IncomingMessage, type Server } from "node:http";

/** A request a fake got: its path, headers, and body. */
export interface Call {
  path: string;
  headers: IncomingMessage["headers"];
  body: Buffer;
}

function body(req: IncomingMessage): Promise<Buffer> {
  return new Promise((ok) => {
    const chunks: Buffer[] = [];
    req.on("data", (c) => chunks.push(c));
    req.on("end", () => ok(Buffer.concat(chunks)));
  });
}

/** GitHub's numeric user id for a login: stable, so the same login is the
 * same account on every sign-in, and below 2^31 however long the login. */
export const githubId = (login: string) => [...login].reduce((h, c) => (h * 31 + c.charCodeAt(0)) % 2147483647, 7);

/** A fake GitHub for OAuth sign-in. Authorize redirects straight back with
 * a code naming who signed in: the `login` query parameter the client adds
 * to the authorize URL, else the browser's `fake_github_login` cookie
 * (dev-browser's --login), else `user()` (control-smoke switches it). No
 * state on the server, so any number of people can sign in at once.
 *
 * With `pick` (dev-control's DEV_CONTROL_LOGINS), authorize without a
 * `login` shows a page instead: a button for `user()` and each of `pick`,
 * and a box for any other login, so a second browser signs in as someone
 * else. */
export function fakeGithub(port: number, host = "127.0.0.1", user: () => string = () => "stranger", pick: string[] = []): Server {
  return createServer(async (req, res) => {
    const u = new URL(req.url!, `http://${host}:${port}`);
    const json = (v: unknown) => res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(v));
    const named = u.searchParams.get("login") || /(?:^|;\s*)fake_github_login=([^;]+)/.exec(req.headers.cookie ?? "")?.[1];
    if (u.pathname === "/login/oauth/authorize" && pick.length && !named) {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8" }).end(picker(u.searchParams, [user(), ...pick]));
    } else if (u.pathname === "/login/oauth/authorize") {
      const back = new URL(u.searchParams.get("redirect_uri")!);
      back.searchParams.set("code", `c0de.${named || user()}`);
      back.searchParams.set("state", u.searchParams.get("state")!);
      res.writeHead(302, { location: back.href }).end();
    } else if (u.pathname === "/login/oauth/access_token") {
      const code = new URLSearchParams((await body(req)).toString()).get("code") ?? "";
      json({ access_token: `gho_${code.replace(/^c0de\./, "")}` });
    } else if (u.pathname === "/user") {
      const login = String(req.headers.authorization ?? "").replace(/^(Bearer|token) gho_/, "");
      json({ id: githubId(login), login });
    } else if (req.method === "POST" && /^\/repos\/[^/]+\/[^/]+\/(issues|pulls)(\/\d+\/(comments|reviews))?$/.test(u.pathname)) {
      // A person's comment, review or new issue (M168): where it is now.
      await body(req);
      const [, , owner, repo, , n] = u.pathname.split("/");
      json({ number: Number(n) || 1, html_url: `https://github.com/${owner}/${repo}/pull/${n || 1}#issuecomment-1` });
    } else if (req.method === "DELETE" && /^\/applications\/[^/]+\/token$/.test(u.pathname)) {
      // A send's user token, revoked (M168).
      await body(req);
      res.writeHead(204).end();
    } else if (req.method === "GET" && /^\/repos\/arugula-salad\/(shipped|planter-apps)\/installation$/.test(u.pathname)) {
      // An App installed on arugula-salad's shipped and planter-apps (M233:
      // planter's, the deploy App), granting what planter's drivers ask for.
      // Any App's JWT is taken.
      json({ id: 77, account: { login: "arugula-salad", id: 1, type: "Organization" },
        permissions: { metadata: "read", contents: "write", pull_requests: "write", workflows: "write" } });
    } else if (req.method === "POST" && u.pathname === "/app/installations/77/access_tokens") {
      // An installation token for the one repository asked for, by name.
      const asked = JSON.parse((await body(req)).toString() || "{}");
      const [name] = asked.repositories ?? [];
      if (typeof name !== "string" || (asked.repositories ?? []).length !== 1) return res.writeHead(422).end();
      json({ token: `ghs_testnet${Math.random().toString(36).slice(2)}`, expires_at: new Date(Date.now() + 3600_000).toISOString(),
        repositories: [{ id: githubId(name), full_name: `arugula-salad/${name}` }] });
    } else if (req.method === "DELETE" && u.pathname === "/installation/token") {
      res.writeHead(204).end();
    } else res.writeHead(404).end();
  }).listen(port, host);
}

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

/** The fake GitHub's "who are you?" page: each button submits the authorize
 * request again with its `login`. */
function picker(q: URLSearchParams, logins: string[]): string {
  const keep = [...q].map(([k, v]) => `<input type="hidden" name="${esc(k)}" value="${esc(v)}">`).join("");
  const buttons = [...new Set(logins)].map((l) => `<button name="login" value="${esc(l)}">Sign in as ${esc(l)}</button>`).join(" ");
  return `<!doctype html><meta charset="utf-8"><meta name="viewport" content="width=device-width">
<title>Fake GitHub</title>
<style>body{font:16px system-ui;margin:3em auto;max-width:28em;padding:0 16px}button,input{font:inherit;padding:.3em .6em}form{margin:1em 0}</style>
<h1>Fake GitHub</h1><p>Sign in to dev-control as:</p>
<form action="/login/oauth/authorize">${keep}${buttons}</form>
<form action="/login/oauth/authorize">${keep}<input name="login" placeholder="another login" aria-label="Login" required> <button>Sign in</button></form>`;
}

/** A fake Stripe: customers, checkout, a subscription with a seat item
 * and a metered item. `calls` holds what control asked for, bodies as
 * forms. */
export function fakeStripe(port: number, host = "127.0.0.1") {
  const calls: { path: string; form: URLSearchParams }[] = [];
  const server = createServer(async (req, res) => {
    const path = req.url!.split("?")[0];
    calls.push({ path, form: new URLSearchParams((await body(req)).toString()) });
    const reply = (v: unknown) => res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(v));
    if (path === "/v1/customers") reply({ id: "cus_1" });
    else if (path === "/v1/checkout/sessions") reply({ id: "cs_1", url: "https://checkout.stripe.test/cs_1" });
    else if (path === "/v1/subscriptions/sub_1")
      reply({ id: "sub_1", items: { data: [{ id: "si_seat", price: { id: "price_seat" } }, { id: "si_min", price: { id: "price_min" } }] } });
    else reply({});
  }).listen(port, host);
  return { server, calls };
}

/** A Web Push endpoint (any path) that takes every notification. */
export function fakePush(port: number, host = "127.0.0.1") {
  const pushed: Call[] = [];
  const server = createServer(async (req, res) => {
    pushed.push({ path: req.url!, headers: req.headers, body: await body(req) });
    res.writeHead(201).end();
  }).listen(port, host);
  return { server, pushed };
}
