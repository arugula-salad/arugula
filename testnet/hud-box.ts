// A hud box as a client of the platform on the testnet (M237): its roster
// from a project's members and its reviews as an object's thread. No SDK,
// only fetch. One JSON object on stdout, exit 0; or why on stderr, exit 1.
//
//   node --experimental-strip-types testnet/hud-box.ts COMMAND --control URL [--via FROM=TO ...] ...
//
//   setup --login L --client ID --port P --state FILE
//               L signs in to control (the fake GitHub) through the box's
//               client, unbound so far; turns chat on; their personal
//               project and an issue in it. Keeps the browser and the ids in
//               FILE: {account, github_id, project, issue}
//   box --state FILE --client ID --secret S --port P --name N --login2 L2 [--hud DIR]
//               With the client bound to the project and given secret S:
//               the box's roster (GET /oauth/hud/members) has L by numeric
//               GitHub id. With DIR, a hud checkout built with hud's
//               platform client: hud's own proxy listens on P, L opens it
//               through /__hud/signin (control, and back with the box's
//               nonce) and is let in with no invite; L2, not a member, is
//               refused; hud's PlatformClient asks to post a review for L.
//               Without DIR, the same calls by hand. The ask posts nothing:
//               L opens control's page it names, an answer from another
//               page is refused, and L presses Post there. Then L reads the
//               issue's thread on the platform, as the page does: the review
//               is there, via the box.
//               {members, admitted, refused, posted, thread}

import http from "node:http";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const argv = process.argv.slice(2);
const cmd = argv.shift();
const opts: Record<string, string> = {};
const via: [string, string][] = [];
for (let i = 0; i < argv.length; i += 2) {
  const [k, v] = [argv[i].replace(/^--/, ""), argv[i + 1]];
  if (k === "via") {
    const at = v.indexOf("=");
    via.push([v.slice(0, at), v.slice(at + 1)]);
  } else opts[k] = v;
}
const need = (k: string) => {
  if (!opts[k]) throw new Error(`--${k} is required`);
  return opts[k];
};
const control = need("control");
const reach = (u: string) => {
  for (const [from, to] of via) if (u.startsWith(from)) return to + u.slice(from.length);
  return u;
};
// The result goes to stdout even once hud's own log lines are sent to stderr.
const out = console.log.bind(console);
const fail = (why: string): never => {
  throw new Error(why);
};

/** A browser: cookies by name, redirects followed by hand. */
class Browser {
  jar: Record<string, string> = {};
  cookie(): string {
    return Object.entries(this.jar).map(([k, v]) => `${k}=${v}`).join("; ");
  }
  keep(res: Response): void {
    for (const c of res.headers.getSetCookie()) {
      const [kv] = c.split(";");
      const at = kv.indexOf("=");
      const [name, value] = [kv.slice(0, at), kv.slice(at + 1)];
      if (value === "" || /Max-Age=0/i.test(c)) delete this.jar[name];
      else this.jar[name] = value;
    }
  }
  async get(url: string): Promise<Response> {
    const res = await fetch(reach(url), { redirect: "manual", headers: { cookie: this.cookie(), accept: "text/html" } });
    this.keep(res);
    return res;
  }
  async send(method: string, url: string, body?: unknown, origin = control): Promise<{ status: number; v: Record<string, unknown> }> {
    const res = await fetch(reach(url), {
      method,
      headers: { cookie: this.cookie(), origin, ...(body !== undefined ? { "content-type": "application/json" } : {}) },
      body: body !== undefined ? JSON.stringify(body) : undefined,
    });
    this.keep(res);
    return { status: res.status, v: (await res.json().catch(() => ({}))) as Record<string, unknown> };
  }
}

/** Signed out: the hub sends `path` through control's page, then a GitHub
 * sign-in as `login` that ends on `path`, followed until it leaves control
 * for an address `stop` takes. */
async function signInThrough(b: Browser, path: string, login: string, stop: (u: string) => boolean): Promise<string> {
  const first = await b.get(control + path);
  const loc = first.headers.get("location") ?? "";
  if (first.status !== 303 || !/^\/#issuer-signin=/.test(loc)) fail(`signed out, ${path} answered ${first.status} ${loc}`);
  let url = `${control}/auth/github?next=${encodeURIComponent(path)}`;
  for (let i = 0; i < 8; i++) {
    const u = new URL(url);
    if (u.pathname === "/login/oauth/authorize") u.searchParams.set("login", login);
    const res = await b.get(u.href);
    const next = res.headers.get("location");
    if (!next) fail(`signing in: ${res.status} at ${u.pathname}: ${(await res.text()).slice(0, 200)}`);
    url = new URL(next!, url).href;
    if (stop(url)) return url;
  }
  return fail("signing in went round too many times");
}

const assertionIn = (url: string) =>
  JSON.parse(Buffer.from((/#hud-forge=hudfs1\.([A-Za-z0-9_-]+)\./.exec(url) ?? fail(`no assertion: ${url}`))[1], "base64url").toString()) as Record<string, unknown>;

async function setup() {
  const [login, client, port] = [need("login"), need("client"), need("port")];
  const back = `http://127.0.0.1:${port}/__hud/signin/back`;
  const b = new Browser();
  const q = new URLSearchParams({ client_id: client, redirect_uri: back, nonce: "testnet-setup-0001" });
  const at = await signInThrough(b, `/oauth/hud?${q}`, login, (u) => u.startsWith(back));
  const p = assertionIn(at);
  if (typeof p.github_id !== "number" || typeof p.account !== "string") fail(`no account or numeric github_id: ${JSON.stringify(p)}`);
  const flag = await b.send("POST", `${control}/api/flags`, { name: "chat", on: true });
  if (flag.status !== 200) fail(`chat on: ${flag.status} ${JSON.stringify(flag.v)}`);
  const projects = await b.send("GET", `${control}/api/projects`);
  const project = ((projects.v.projects ?? []) as { id: string; team: unknown }[]).find((x) => x.team === null)?.id ?? fail(`no personal project: ${JSON.stringify(projects.v)}`);
  const issue = await b.send("POST", `${control}/api/projects/${project}/issues`, { title: "The header overlaps the logo" });
  if (issue.status !== 200) fail(`issue: ${issue.status} ${JSON.stringify(issue.v)}`);
  const state = { jar: b.jar, login, account: p.account, github_id: p.github_id, project, issue: issue.v.id };
  writeFileSync(need("state"), JSON.stringify(state));
  return { account: p.account, github_id: p.github_id, project, issue: issue.v.id };
}

async function box() {
  const state = JSON.parse(readFileSync(need("state"), "utf8"));
  const [client, secret, port, name, login2] = [need("client"), need("secret"), Number(need("port")), need("name"), need("login2")];
  const basic = `Basic ${Buffer.from(`${client}:${secret}`).toString("base64")}`;
  const asBox = async (method: string, path: string, body?: unknown) => {
    const r = await fetch(reach(control + path), {
      method,
      headers: { authorization: basic, ...(body ? { "content-type": "application/json" } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
    return { status: r.status, v: (await r.json().catch(() => ({}))) as Record<string, unknown> };
  };
  // The roster: the project's members, by numeric GitHub id.
  const roster = await asBox("GET", "/oauth/hud/members");
  if (roster.status !== 200 || roster.v.project !== state.project) fail(`the box's roster: ${roster.status} ${JSON.stringify(roster.v)}`);
  const members = roster.v.members as { account: string; github_id: number | null }[];
  if (!members.some((m) => m.account === state.account && m.github_id === state.github_id)) fail(`the roster lacks the member: ${JSON.stringify(members)}`);

  const b = new Browser();
  b.jar = state.jar;
  const boxUrl = `http://127.0.0.1:${port}`;
  const review = { route: "/", note: "From the testnet.", comments: [{ text: "The header overlaps the logo.", elements: [{ tag: "header", text: "Notes" }] }] };
  let admitted: unknown;
  let refused: unknown;
  let posted: unknown;
  let text: string;
  let confirmUrl: string | undefined;
  if (opts.hud) {
    // hud's own code, from a checkout built with its platform client.
    // Its log lines go to stderr: stdout is this script's one JSON object.
    console.log = console.error;
    const dist = join(opts.hud, "packages/daemon/dist");
    const load = (p: string) => import(pathToFileURL(join(dist, p)).href);
    const [{ startHudProxy }, { ForgeSignin, JwksKeys }, { PlatformClient, reviewText }, { createIdentitySecret, mintInvite, PlayerRegistry }] = await Promise.all([
      load("proxy/server.js"),
      load("proxy/forge-signin.js"),
      load("proxy/platform.js"),
      load("proxy/identity.js"),
    ]);
    // The box reaches control as the browser does on the testnet.
    const boxFetch = ((u: string | URL, init?: RequestInit) => fetch(reach(String(u)), init)) as typeof fetch;
    const dev = http.createServer((_q, s) => s.writeHead(200, { "content-type": "text/plain" }).end("the app"));
    const listen = (s: http.Server) => new Promise<number>((r) => s.listen(0, "127.0.0.1", () => r((s.address() as { port: number }).port)));
    const devPort = await listen(dev);
    const config = { url: control, clientId: client, publicUrl: boxUrl };
    const signin = new ForgeSignin({ jwks: new JwksKeys(`${control}/.well-known/jwks.json`, boxFetch), audience: client, signinUrl: `${boxUrl}/__hud/signin` });
    const platform = new PlatformClient(config, secret, boxFetch);
    const identitySecret = createIdentitySecret();
    const players = new PlayerRegistry();
    const proxy = startHudProxy({
      host: "127.0.0.1",
      port,
      devPort,
      ingestPort: devPort,
      humanPort: devPort,
      identitySecret,
      ownerInvite: mintInvite(identitySecret, "owner"),
      players,
      store: { recordClientSignal() {} },
      shell: false,
      platformSignin: { config, signin, client: platform },
    });
    await new Promise((r) => proxy.server.once("listening", r));
    try {
      // The member, signed in to control already, opens the box.
      const viaBox = async (br: Browser, login?: string) => {
        const start = await br.get(`${boxUrl}/__hud/signin`);
        const to = start.headers.get("location") ?? fail(`/__hud/signin: ${start.status}`);
        const back = (u: string) => u.startsWith(`${boxUrl}/__hud/signin/back`);
        let at: string;
        if (login) {
          at = await signInThrough(br, to.slice(control.length), login, back);
        } else {
          const hub = await br.get(to);
          at = hub.headers.get("location") ?? fail(`/oauth/hud: ${hub.status} ${await hub.text()}`);
        }
        if (!back(at)) fail(`the hub sent the browser to ${at}`);
        const page = await br.get(at);
        if (page.status !== 200) fail(`the box's back page: ${page.status}`);
        const a = new URLSearchParams(new URL(at).hash.slice(1)).get("hud-forge");
        return br.send("POST", `${boxUrl}/__hud/signin`, { assertion: a }, boxUrl);
      };
      const mine = await viaBox(b);
      if (mine.status !== 200) fail(`the member wasn't let in: ${mine.status} ${JSON.stringify(mine.v)}`);
      const app = await fetch(`${boxUrl}/`, { headers: { cookie: b.cookie(), accept: "text/plain" } });
      if (app.status !== 200) fail(`the member's cookie didn't open the app: ${app.status}`);
      const row = players.findMember(state.github_id) ?? fail("no member row by numeric GitHub id");
      admitted = { principal: mine.v.principal, role: row.role, github_id: row.member.githubId };
      const stranger = await viaBox(new Browser(), login2);
      if (stranger.status !== 403 || stranger.v.reason !== "not-a-member") fail(`a non-member: ${stranger.status} ${JSON.stringify(stranger.v)}`);
      refused = stranger.v.reason;
      text = reviewText(review);
      const p = await platform.askPost(state.issue, state.account, text);
      if (!p.ok) fail(`hud's ask to post: ${JSON.stringify(p)}`);
      confirmUrl = p.confirmUrl;
    } finally {
      proxy.server.closeAllConnections();
      proxy.server.close();
      dev.close();
    }
  } else {
    // By hand: the member signs in to the box through the hub, then the box posts.
    const q = new URLSearchParams({ client_id: client, redirect_uri: `${boxUrl}/__hud/signin/back`, nonce: "testnet-box-00001" });
    const hub = await b.get(`${control}/oauth/hud?${q}`);
    const p = assertionIn(hub.headers.get("location") ?? fail(`/oauth/hud: ${hub.status}`));
    admitted = { by_hand: true, github_id: p.github_id };
    const b2 = new Browser();
    const at2 = await signInThrough(b2, `/oauth/hud?${q}`, login2, (u) => u.startsWith(boxUrl));
    const p2 = assertionIn(at2);
    if (members.some((m) => m.github_id === p2.github_id)) fail("the non-member is on the roster");
    refused = "not on the roster";
    text = `Review on /:\n1. The header overlaps the logo. (on header "Notes")\n\nFrom the testnet.`;
    const r = await asBox("POST", "/oauth/hud/posts", { target: state.issue, author: state.account, text });
    if (r.status !== 200 || typeof r.v.confirm_url !== "string") fail(`the ask to post: ${r.status} ${JSON.stringify(r.v)}`);
    confirmUrl = r.v.confirm_url as string;
  }
  // Asking posted nothing; the member posts it on control's own page.
  const room = await b.send("POST", `${control}/api/chat/targets`, { kind: "object", target: state.issue });
  if (room.status !== 200) fail(`the issue's thread: ${room.status} ${JSON.stringify(room.v)}`);
  const thread = async () => ((await b.send("GET", `${control}/api/chat/rooms/${room.v.id}/messages`)).v.messages ?? []) as { author: string; text: string; via?: string }[];
  if ((await thread()).some((x) => x.text === text)) fail("the box's ask posted the review by itself");
  if (!confirmUrl!.startsWith(`${control}/oauth/hud/post?id=`)) fail(`control's page is somewhere else: ${confirmUrl}`);
  const shown = await b.get(confirmUrl!);
  const html = await shown.text();
  if (shown.status !== 200 || !html.includes("The header overlaps the logo.") || !/frame-ancestors 'none'/.test(shown.headers.get("content-security-policy") ?? "")) fail(`control's page: ${shown.status} ${html.slice(0, 300)}`);
  const id = new URL(confirmUrl!).searchParams.get("id")!;
  const own = new URL(reach(control)).origin;
  const answer = (origin: string) =>
    fetch(reach(`${control}/oauth/hud/post`), {
      method: "POST",
      headers: { cookie: b.cookie(), origin, "sec-fetch-site": origin === own ? "same-origin" : "cross-site", "content-type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({ id, decision: "post" }),
    });
  const elsewhere = await answer(boxUrl);
  if (elsewhere.status !== 403) fail(`an answer from the box's origin: ${elsewhere.status}`);
  const pressed = await answer(own);
  if (pressed.status !== 200) fail(`Post on control's page: ${pressed.status} ${(await pressed.text()).slice(0, 300)}`);
  posted = { asked: true, confirmed_on: "control's page", refused_from_elsewhere: elsewhere.status };
  // As the member reads it on the platform page.
  const m = (await thread()).find((x) => x.text === text);
  if (!m) fail("the review isn't in the issue's thread");
  if (m!.author !== state.account || m!.via !== name) fail(`the review's author or via: ${JSON.stringify(m)}`);
  return { members: members.length, admitted, refused, posted, thread: { room: room.v.id, author: m!.author, via: m!.via } };
}

const run = { setup, box }[cmd ?? ""];
if (!run) {
  console.error("usage: hud-box.ts setup|box --control URL ...");
  process.exit(2);
}
run()
  .then((r) => out(JSON.stringify(r)))
  .catch((e) => {
    console.error(`hud-box.ts ${cmd}: ${e instanceof Error ? e.message : e}`);
    process.exit(1);
  });
