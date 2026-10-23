// studio's own lobby as a client of the platform on the testnet (M236,
// Arugula #34), driven as its people would: one JSON object on stdout,
// exit 0; or why on stderr, exit 1. The lobby runs from a studio checkout
// (testnet/studio-run.mjs, in a container on the stack's networks); this is
// the browser and the box.
//
//   node --experimental-strip-types testnet/studio.ts --control URL [--via FROM=TO ...] --studio URL --login PREFIX [--break]
//
//   1. Jake and Alex sign in to control (the fake GitHub); Jake makes a team
//      and Alex joins it as an editor (--break: Alex is never admitted).
//   2. Each signs in to the lobby with Arugula: the hub's consent screen
//      once, back with a code, and the lobby makes an account linked to
//      theirs by numeric GitHub id. Again from a fresh browser: the same.
//   3. On the platform page, each lets studio see their projects and use
//      Claude for them (Settings); the linked product lists the switch.
//   4. Jake makes an app in the team's project; Alex, who didn't make it,
//      sees it by its project as an editor and opens it. The project's
//      settings list it.
//   5. Its box claims it, and its egress secret takes its value from the
//      project's own secret, which Jake sets on the platform page. Saving
//      it in the lobby asks the platform for this app and this host; it's
//      refused until Jake allows that on the platform page, and after it
//      the fake API gets the value, never the box. The read is counted.
//   6. The box's Claude call goes through the platform's broker as studio,
//      for Jake: the platform's key pays, and Anthropic never sees the
//      box's token or studio's credential.

import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";
import { Device } from "../web/fixtures/device.ts";

const argv = process.argv.slice(2);
const opts: Record<string, string> = {};
const via: Record<string, string> = {};
for (let i = 0; i < argv.length; i++) {
  const k = argv[i].replace(/^--/, "");
  if (k === "break") {
    opts.break = "1";
    continue;
  }
  const v = argv[++i];
  if (k === "via") {
    const at = v.indexOf("=");
    via[v.slice(0, at)] = v.slice(at + 1);
  } else opts[k] = v;
}
const need = (k: string) => opts[k] ?? fail(`--${k} is required`);
const control = need("control").replace(/\/$/, "");
const studio = need("studio").replace(/\/$/, "");
const broken = Boolean(opts.break);
const reach = (u: string) => {
  for (const [from, to] of Object.entries(via)) if (u.startsWith(from)) return to + u.slice(from.length);
  return u;
};
function fail(why: string): never {
  throw new Error(why);
}
const expect = (ok: unknown, what: string, got?: unknown) => {
  if (!ok) fail(`${what}${got === undefined ? "" : `: ${typeof got === "string" ? got.slice(0, 400) : JSON.stringify(got).slice(0, 400)}`}`);
};

/** Test values, set on the stack too (compose.yaml). */
const PLATFORM_KEY = "sk-ant-api03-testnet-platform-key-0123456789"; // gitleaks:allow: a test value
const CREDENTIAL = "testnet-studio-client-credential";
const STRIPE = `sk_test_testnet_${randomBytes(8).toString("hex")}`;

/** A browser at the lobby: its cookies, and control's session for the hub. */
class Browser {
  jar: Record<string, string> = {};
  d: Device;
  constructor(d: Device) {
    this.d = d;
  }
  private keep(res: Response) {
    for (const c of res.headers.getSetCookie()) {
      const [kv, ...attrs] = c.split(";");
      const at = kv.indexOf("=");
      if (attrs.some((a) => /max-age=0/i.test(a))) delete this.jar[kv.slice(0, at)];
      else this.jar[kv.slice(0, at)] = kv.slice(at + 1);
    }
  }
  /** The lobby: `{status, json, location}`. */
  async at(method: string, path: string, body?: unknown) {
    const res = await fetch(studio + path, {
      method,
      redirect: "manual",
      headers: {
        origin: studio,
        cookie: Object.entries(this.jar).map(([k, v]) => `${k}=${v}`).join("; "),
        ...(body === undefined ? {} : { "content-type": "application/json" }),
      },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    this.keep(res);
    const text = await res.text();
    let json: any = text;
    try {
      json = JSON.parse(text);
    } catch {}
    return { status: res.status, json, location: res.headers.get("location") };
  }
  /** "Sign in with Arugula": the lobby's POST, the hub (with control's
   * session; the consent screen the first time), the lobby's callback.
   * How many consent screens it saw. */
  async signIn(): Promise<number> {
    const started = await this.at("POST", "/api/auth/arugula/signin", {});
    expect(started.status === 200, "the lobby starts a sign-in with Arugula", started.json);
    let url: string = started.json.url;
    let consents = 0;
    for (let i = 0; i < 6; i++) {
      if (url.startsWith(studio)) break;
      let res = await fetch(reach(url), { redirect: "manual", headers: { cookie: this.d.cookie } });
      if (res.status === 200 && new URL(url).pathname === "/oauth/authorize") {
        const page = await res.text();
        const ticket = /name="ticket" value="([^"]+)"/.exec(page)?.[1] ?? fail(`no consent ticket: ${page.slice(0, 200)}`);
        expect(page.includes("Studio"), "the consent screen names the product", page.slice(0, 300));
        res = await fetch(reach(`${control}/oauth/consent`), {
          method: "POST",
          redirect: "manual",
          headers: { cookie: this.d.cookie, origin: new URL(reach(control)).origin, "content-type": "application/x-www-form-urlencoded" },
          body: new URLSearchParams({ ticket, decision: "allow" }).toString(),
        });
        consents++;
      }
      url = new URL(res.headers.get("location") ?? fail(`at the hub: ${res.status} ${(await res.text()).slice(0, 200)}`), url).href;
    }
    expect(url.startsWith(`${studio}/api/auth/arugula/callback?`), "the hub sends the browser back to the lobby", url);
    const back = await this.at("GET", url.slice(studio.length));
    expect(back.status === 302 && back.location === "/", "the lobby signs them in", back.json);
    return consents;
  }
}

/** What the fake Anthropic (fakes:9004) saw: each request's path, the key it carried, and every header's value. */
function seen(): { path: string; key: string; headers: string[]; values: string }[] {
  const container = `${process.env.COMPOSE_PROJECT_NAME ?? "arugula-testnet"}-fakes`;
  const js = "fetch('http://127.0.0.1:9004/_testnet/seen').then((r) => r.text()).then((t) => process.stdout.write(t))";
  return JSON.parse(execFileSync("docker", ["exec", container, "node", "-e", js], { encoding: "utf8" }));
}

async function main() {
  const login = need("login");
  // 1.
  const jake = await Device.signIn({ control, via, login: `${login}-jake` });
  const alex = await Device.signIn({ control, via, login: `${login}-alex` });
  const team = await jake.createTeam("studio apps");
  const seed = await jake.invite(team, "editor");
  await alex.acceptInvite(team, seed);
  if (!broken) expect((await jake.admitAll(team)) === 1, "Alex admitted as an editor");
  const project = (await jake.project(team)).id;

  // 2.
  const jb = new Browser(jake);
  expect((await jb.signIn()) === 1, "Jake's first sign-in through studio asks the hub's consent once");
  const jakeMe = (await jb.at("GET", "/api/me")).json;
  expect(jakeMe.user?.platformAccount === jake.account, "Jake's studio account is linked to his Arugula account", jakeMe.user);
  expect(typeof jakeMe.user.githubId === "number" && jakeMe.user.passkeys === 0, "by numeric GitHub id, with no passkey", jakeMe.user);
  const again = new Browser(jake);
  expect((await again.signIn()) === 0, "a second sign-in asks nothing");
  expect((await again.at("GET", "/api/me")).json.user?.name === jakeMe.user.name, "and finds the same account");
  const ab = new Browser(alex);
  await ab.signIn();

  // 3.
  const before = (await ab.at("GET", "/api/me")).json;
  expect(before.platform?.projectsWhy === "consent", "until Alex lets it, studio can't see his projects", before.platform);
  for (const d of [jake, alex]) {
    await d.api("/api/linked-products/studio/projects", undefined, "PUT");
    await d.api("/api/inference/clients/studio/consent", undefined, "PUT");
  }
  const linked = await jake.api<{ links: { client: string; user?: string; projects?: boolean }[] }>("/api/linked-products");
  expect(linked.links.some((l) => l.client === "studio" && l.user && l.projects === true), "the platform page lists studio, linked, with its switch on", linked);

  // 4.
  const jm = (await jb.at("GET", "/api/me")).json;
  expect(jm.platform?.projects?.some((p: { id: string; role: string }) => p.id === project && p.role === "owner"), "studio sees Jake's team project", jm.platform);
  expect(jm.canCreate === true, "Claude through Arugula is enough to make an app", jm);
  const made = await jb.at("POST", "/api/apps", { title: "Team board", project });
  expect(made.status === 201 && made.json.project === project, "Jake makes an app in the team's project", made.json);
  const app: string = made.json.name;
  const am = (await ab.at("GET", "/api/me")).json;
  const his = (am.apps ?? []).find((a: { name: string }) => a.name === app);
  expect(his?.role === "editor" && his.mine === false, "Alex sees Jake's app by its project, as an editor", am.apps ?? am);
  const opened = await ab.at("POST", `/api/apps/${app}/open`, {});
  expect(opened.status === 200 && /\/__claim\?/.test(opened.json.url ?? ""), "Alex opens an app he didn't make", opened.json);
  const listed = await alex.api<{ apps: { app: string; client: string }[] }>(`/api/projects/${project}/clients`);
  expect(listed.apps.some((a) => a.app === app && a.client === "studio"), "the project's settings list studio's app", listed);

  // 5.
  const box = `studio_${randomBytes(32).toString("hex")}`;
  const nonce = new URL(made.json.url).searchParams.get("n");
  const enrolled = await fetch(`${studio}/api/enroll`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ name: app, nonce, token: box, enter: randomBytes(32).toString("hex") }),
  });
  expect(enrolled.ok, "the box claims the app", await enrolled.text());
  const caps = [
    { name: "inference", broker: "lobby", scope: ["agent", "decide"] },
    { name: "egress", broker: "lobby", scope: ["STRIPE"] },
  ];
  const declared = await fetch(`${studio}/api/box/declaration`, { method: "POST", headers: { authorization: `Bearer ${box}`, "content-type": "application/json" }, body: JSON.stringify({ capabilities: caps }) });
  expect(declared.ok, "the box declares what it uses", declared.status);
  await jake.api(`/api/projects/${project}/secrets/STRIPE_TEST`, { value: STRIPE }, "PUT");
  const secret = await jb.at("PUT", `/api/apps/${app}/secrets/STRIPE`, { host: "http://fakes:9004", from: { value: "STRIPE_TEST" } });
  expect(secret.status === 200, "Jake, an owner of the project, takes STRIPE's value from it", secret.json);
  const egress = () => fetch(`${studio}/egress/STRIPE/v1/models`, { headers: { "x-api-key": box } });
  const refused = await egress();
  expect(refused.status === 403 && /haven't let studio send STRIPE_TEST to http:\/\/fakes:9004/.test(await refused.text()), "asked, not granted: refused", refused.status);
  const asks = await jake.api<{ grants: { app: string; name: string; host: string; granted_at: number | null }[] }>(`/api/projects/${project}/clients`);
  expect(asks.grants.some((g) => g.app === app && g.name === "STRIPE_TEST" && g.host === "http://fakes:9004" && g.granted_at === null), "studio asked, for this app and this host", asks);
  await jake.api(`/api/projects/${project}/clients/studio/apps/${app}/secrets/STRIPE_TEST`, undefined, "PUT");
  const mark = seen().length;
  const used = await egress();
  expect(used.status === 200, "granted: the box's request goes out", `${used.status} ${await used.clone().text()}`);
  const out = seen().slice(mark);
  expect(out.length === 1 && out[0].key === STRIPE, "the API got the project's secret, as the owner chose", out.map((s) => s.key.slice(0, 16)));
  expect(!out[0].values.includes(box), "and never the box's token");
  const grants = await jake.api<{ grants: { name: string; reads: number }[]; events: { kind: string }[] }>(`/api/projects/${project}/clients`);
  const g = grants.grants.find((x) => x.name === "STRIPE_TEST");
  expect(g && g.reads >= 1 && grants.events.some((e) => e.kind === "read"), "the project's owners see the read", grants);

  // 6.
  const mark2 = seen().length;
  const llm = await fetch(`${studio}/llm/anthropic/v1/messages`, {
    method: "POST",
    headers: { "x-api-key": box, "anthropic-version": "2023-06-01", "content-type": "application/json" },
    body: JSON.stringify({ model: "claude-opus-5", max_tokens: 10, messages: [{ role: "user", content: "hi" }] }),
  });
  const said = await llm.text();
  expect(llm.status === 200 && llm.headers.get("chant-payer") === "shared", "the box's Claude call goes through the platform's broker, on its key", `${llm.status} ${llm.headers.get("chant-payer")} ${said}`);
  const upstream = seen().slice(mark2);
  expect(upstream.length === 1 && upstream[0].key === PLATFORM_KEY, "Anthropic saw the platform's key", upstream.map((s) => s.key.slice(0, 16)));
  for (const leak of [box, CREDENTIAL, STRIPE]) expect(!upstream[0].values.includes(leak), "nothing of the box's or studio's reached Anthropic");
  const month = (await jb.at("GET", "/api/me")).json.platform?.claude;
  return {
    project,
    app,
    jake: jake.account,
    alex: alex.account,
    teammate_opened: true,
    egress: "project secret",
    reads: g!.reads,
    broker: llm.headers.get("chant-payer"),
    month,
  };
}

main().then(
  (v) => {
    console.log(JSON.stringify(v));
    process.exit(0);
  },
  (e: Error) => {
    console.error(`studio: ${e.message}`);
    process.exit(1);
  },
);
