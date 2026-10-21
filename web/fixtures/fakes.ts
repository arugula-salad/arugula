// The outside services arugula-control talks to, faked: GitHub sign-in,
// Stripe, a Web Push endpoint and Managoat (Cloud workspaces). `web/control-smoke.ts` starts them in its
// own process; the test stack's `control` profile (testnet/) runs them in a
// container with `fakes-serve.ts`. Each records what it was sent, for the
// test to check.

import { readFileSync, writeFileSync } from "node:fs";
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

/** One turn of a fake Managoat conversation, from its prompt: an approval
 * it asks for first (its summary), the blocks it answers with (after the
 * approval, if it asks), and whether it changes the repository (the
 * machine's diff and git status then show `fakeChange`). */
export interface FakeTurn {
  asks?: string;
  /** Managoat's blocks, one event each, as `?blocks=true` sends them: a
   * message streams as many `text` chunks. */
  blocks: { kind: string; body?: unknown; id?: string; name?: string; summary?: string; tool_id?: string }[];
  writes?: boolean;
}

/** A reply streamed as Managoat sends one: text in chunks that cut words
 * and a code fence apart, and a tool call whose input arrives empty (`{}`)
 * before its result. */
export const streamedTurn: FakeTurn["blocks"] = [
  { kind: "text", body: "I" }, { kind: "text", body: "'ll" }, { kind: "text", body: " run" }, { kind: "text", body: " `" },
  { kind: "text", body: "pwd" }, { kind: "text", body: "` first." },
  { kind: "tool_use", id: "tool1", name: "Terminal", summary: "pwd", body: "{}" },
  { kind: "tool_result", tool_id: "tool1", body: "/home/sprite/app" },
  { kind: "text", body: "hello from cla" }, { kind: "text", body: "ude\n\n```sh\necho" }, { kind: "text", body: " hi\n```\n" },
];

/** The e2e's turns: "Run the tests" asks first, "Write the guide" writes,
 * "Stream it" streams. */
const testTurn = (prompt: string): FakeTurn => ({
  asks: prompt === "Run the tests" ? "Run the tests?" : undefined,
  blocks: prompt === "Stream it" ? streamedTurn : [{ kind: "text", body: prompt === "Run the tests" ? "Tests pass." : "Docs written." }],
  writes: prompt === "Write the guide",
});

/** What a turn that writes leaves in a repository: README.md changed and
 * docs/guide.md new. */
export const fakeChange = {
  diff: "diff --git a/README.md b/README.md\nindex 1..2 100644\n--- a/README.md\n+++ b/README.md\n@@ -1 +1,2 @@\n # app\n+Docs for the app.\n",
  file: { name: "docs/guide.md", content: "# Guide\nStart." },
};

export interface FakeManagoatOptions {
  /** Why a workspace's setup fails, or null when it doesn't: from its
   * environment as the setup read it, and which setup it is (1 the first,
   * then one more for each rebuild or retry). Asked on every read. */
  fails?: (environment: Record<string, unknown>, setup: number) => string | null;
  turn?: (prompt: string) => FakeTurn;
  /** Whether a rebuild of this workspace finds no machine free: Managoat
   * answers 503 fleet_full, and control keeps asking (M120). */
  full?: (workspace: string) => boolean;
  /** A JSON file the state is kept in between runs (dev-control's: control's
   * database outlives the process, so its workspaces must too). */
  file?: string;
  /** Called with each request before it's answered, and awaited: the
   * testnet's handover claim holds one (M144). `from` is the caller's
   * address. */
  gate?: (method: string, path: string, from: string) => Promise<void>;
}

/** The environment fields a workspace's setup reads: the workspace is
 * behind in those that changed since (M117). */
const buildFields = ["repositories", "packages", "setup_script", "setup_timeout_seconds", "env_vars", "networking_type", "networking_config"];

interface Turn {
  prompt: string;
  answered: boolean;
  /** The option its approval was answered with. */
  option?: string;
  interrupted?: boolean;
  /** Its change was on a disk a rebuild or retry wiped. */
  wiped?: boolean;
}

/** A fake Managoat (S40): environments, vaults and their secrets, Cloud
 * workspaces' machines that set up over their first reads and then are
 * ready (or fail), rebuilds and retries, the machine's reads (diff, git
 * status, a file), agents, and conversations whose turns come from `turn`.
 * `state` is what it holds, for a test to check; `rebuilds` and `retries`
 * what each was asked with. Every id is the fake's own: env1, ws1,
 * tabconv1. */
export function fakeManagoat(port = 0, host = "127.0.0.1", options: FakeManagoatOptions = {}) {
  const turnOf = options.turn ?? testTurn;
  const fails = options.fails ?? (() => null);
  const full = options.full ?? (() => false);
  const empty = () => ({
    secret: "",
    environments: {} as Record<string, Record<string, unknown> & { revision: number }>,
    // Each machine: its reads since it was (re)built, which setup it is,
    // and the environment as that setup read it.
    workspaces: {} as Record<string, { name: string; environment: string; reads: number; setups: number; built: Record<string, unknown> & { revision: number } }>,
    agents: 0,
    conversations: {} as Record<string, { workspace: string; turns: Turn[]; terminated: boolean; labels: unknown }>,
    rebuilds: [] as unknown[],
    retries: [] as unknown[],
    machineReads: [] as string[],
  });
  let state = empty();
  if (options.file) {
    try {
      state = { ...state, ...JSON.parse(readFileSync(options.file, "utf8")) };
    } catch {
      // a first run
    }
  }
  const save = () => options.file && writeFileSync(options.file, JSON.stringify(state, null, 1));
  const changed = (w: (typeof state.workspaces)[string]) => {
    const env = state.environments[w.environment] ?? {};
    return buildFields.filter((f) => JSON.stringify(env[f]) !== JSON.stringify(w.built[f]));
  };
  const machine = (id: string) => {
    const w = state.workspaces[id];
    const ready = w.reads > 2;
    const revision = state.environments[w.environment]?.revision ?? 1;
    const failed = ready ? fails(w.built, w.setups) : null;
    if (failed) return { id, name: w.name, environment_id: w.environment, status: "failed", sandbox_status: null, setup_step: null, error: failed, revision: `rev${revision}`, built_from_revision: null, stale: false, changed_fields: [] };
    const behind = ready ? changed(w) : [];
    return {
      id, name: w.name, environment_id: w.environment,
      status: ready ? "ready" : "provisioning", sandbox_status: ready ? "ready" : "starting", error: null,
      // Managoat v0.39.0: the step a setup is running, while it runs.
      setup_step: ready ? null : (["machine", "packages", "setup_script"][w.reads] ?? "setup_script"),
      revision: `rev${revision}`, built_from_revision: ready ? `rev${w.built.revision}` : null, stale: behind.length > 0, changed_fields: behind,
    };
  };
  // A conversation's events: each turn's start, its approval while it asks,
  // its answer once it may.
  const events = (c: (typeof state.conversations)[string]) => {
    const out: Record<string, unknown>[] = [];
    let id = 0;
    c.turns.forEach((t, n) => {
      const turn = turnOf(t.prompt);
      out.push({ id: ++id, stage: "turn", state: "started", blocks: [{ kind: "prompt", body: t.prompt }] });
      const request = n ? `approval${n}` : "approval";
      if (turn.asks) out.push({ id: ++id, blocks: [{ kind: "permission_request", request_id: request, summary: turn.asks, options: [{ optionId: "allow", name: "Allow once" }, { optionId: "always", name: "Allow always" }, { optionId: "deny", name: "Deny" }] }] });
      if (turn.asks && t.answered) out.push({ id: ++id, stage: "request", state: "done", data: JSON.stringify({ request_id: request, kind: "permission", outcome: "answered", option_id: t.option ?? "allow" }) });
      if (t.interrupted) out.push({ id: ++id, stage: "turn", state: "interrupted", blocks: [{ kind: "text", body: "Interrupted." }] });
      else if (!turn.asks || t.answered) for (const b of turn.blocks) out.push({ id: ++id, blocks: [b] });
    });
    return out;
  };
  const waiting = (c: (typeof state.conversations)[string]) => {
    const t = c.turns.at(-1);
    return !!t && !c.terminated && !!turnOf(t.prompt).asks && !t.answered && !t.interrupted;
  };
  const wrote = (workspace: string) =>
    Object.values(state.conversations).some((c) => c.workspace === workspace && c.turns.some((t) => !t.wiped && turnOf(t.prompt).writes && (t.answered || !turnOf(t.prompt).asks)));

  const server = createServer(async (req, res) => {
    const raw = await body(req);
    const b = raw.length ? JSON.parse(raw.toString()) : {};
    const url = new URL(req.url!, "http://managoat");
    const path = url.pathname;
    if (options.gate) await options.gate(req.method!, path, req.socket.remoteAddress ?? "");
    const reply = (data: unknown, status = 200) => {
      save();
      res.writeHead(status, { "content-type": "application/json" }).end(JSON.stringify({ data }));
    };
    const missing = () => res.writeHead(404).end();
    const parts = path.split("/").slice(1); // api, collection, id, rest…
    if (parts[0] !== "api") return missing();
    const [, collection, id, ...rest] = parts;
    if (req.method === "DELETE") {
      if (collection === "workspaces" && id && !rest.length) delete state.workspaces[id];
      save();
      return res.writeHead(204).end();
    }
    if (collection === "vaults") {
      if (!id) return reply(req.method === "POST" ? { id: "vault" } : []);
      if (rest[0] === "secrets") {
        state.secret = b.value;
        return reply({ key: b.key });
      }
      return missing();
    }
    if (collection === "environments") {
      if (!id && req.method === "POST") {
        const env = `env${Object.keys(state.environments).length + 1}`;
        state.environments[env] = { setup_timeout_seconds: 120, networking_type: "unrestricted", networking_config: {}, ...b, revision: 1 };
        return reply({ id: env, revision: "rev1" });
      }
      if (!id) return reply(Object.entries(state.environments).map(([id, e]) => ({ id, metadata: e.metadata })));
      const env = state.environments[id];
      if (!env) return missing();
      if (req.method === "PATCH") {
        const next = { ...env, ...b };
        if (buildFields.some((f) => JSON.stringify(next[f]) !== JSON.stringify(env[f]))) next.revision = env.revision + 1;
        state.environments[id] = next;
      }
      return reply({ id, revision: `rev${state.environments[id].revision}` });
    }
    if (collection === "workspaces") {
      if (!id && req.method === "POST") {
        const ws = `ws${Object.keys(state.workspaces).length + 1}`;
        state.workspaces[ws] = { name: b.name, environment: b.environment_id, reads: 0, setups: 1, built: { ...state.environments[b.environment_id] } };
        return reply({ ...machine(ws), vault_id: b.vault_id, status: "provisioning", sandbox_status: "pending" });
      }
      if (!id) return reply(Object.keys(state.workspaces).map(machine));
      const w = state.workspaces[id];
      if (!w) return missing();
      if (!rest.length) {
        w.reads++;
        return reply(machine(id));
      }
      // A new machine from the environment as it is now, set up over the
      // next reads. A rebuild is refused while a turn runs, unless it
      // interrupts them.
      if ((rest[0] === "rebuild" || rest[0] === "retry") && req.method === "POST") {
        (rest[0] === "retry" ? state.retries : state.rebuilds).push(b);
        if (rest[0] === "rebuild" && full(id)) {
          save();
          return res.writeHead(503, { "content-type": "application/json" }).end(JSON.stringify({ error: "fleet_full", message: "no machine free" }));
        }
        const running = Object.entries(state.conversations).filter(([, c]) => c.workspace === id && waiting(c)).map(([id]) => id);
        if (rest[0] === "rebuild" && running.length && !b.interrupt) {
          save();
          return res.writeHead(409, { "content-type": "application/json" }).end(JSON.stringify({ error: "turns_running", message: "refused", conversation_ids: running }));
        }
        for (const c of running) state.conversations[c].turns.at(-1)!.interrupted = true;
        for (const c of Object.values(state.conversations)) if (c.workspace === id) for (const t of c.turns) t.wiped = true;
        w.built = { ...state.environments[w.environment] };
        w.reads = 0;
        w.setups++;
        return reply({ ...machine(id), status: "provisioning", sandbox_status: "pending" }, 202);
      }
      // What the last setup printed (v0.39.0): kept by Managoat, so it reads
      // whatever the machine is doing. A failed setup's script printed its
      // error.
      if (rest[0] === "setup-log" && req.method === "GET") {
        const m = machine(id) as { status: string; error: string | null };
        if (m.status === "provisioning" && w.reads === 0) return reply(null);
        const failed = m.status === "failed";
        const step = (name: string, status: string, output: string) => ({ step: name, status, started_at: "2026-10-19T12:00:00Z", finished_at: status === "running" ? null : "2026-10-19T12:00:05Z", output, omitted_lines: 0, omitted_bytes: 0 });
        const steps = [
          step("machine", "succeeded", ""),
          step("packages", "succeeded", "Reading package lists...\nSetting up jq (1.7.1-3)\n"),
          step("repositories", "succeeded", "Cloning into '/home/sprite/app'...\n"),
          step("setup_script", failed ? "failed" : m.status === "ready" ? "succeeded" : "running", failed ? `${(m.error ?? "").split(":").slice(1).join(":").trim()}\n` : "done\n"),
        ];
        return reply({ workspace_id: id, sandbox_id: `sandbox-${id}`, status: failed ? "failed" : m.status === "ready" ? "succeeded" : "running", step: failed ? "setup_script" : null, started_at: "2026-10-19T12:00:00Z", finished_at: "2026-10-19T12:00:09Z", steps });
      }
      // The machine's reads (M118): the change a writing turn made, once it ran.
      if (["diff", "git-status", "file"].includes(rest[0])) {
        state.machineReads.push(`${path}${url.search}`);
        const root = url.searchParams.get("path");
        const changes = wrote(id);
        if (rest[0] === "diff") return reply({ path: root, repo_root: root, staged: false, ref: url.searchParams.get("ref"), truncated: false, diff: changes ? fakeChange.diff : "" });
        if (rest[0] === "git-status")
          return reply({
            path: root, repo_root: root, branch: "main", untracked: "all", truncated: false,
            entries: changes ? [{ path: "README.md", index: "unchanged", worktree: "modified", renamed_from: null }, { path: fakeChange.file.name, index: "untracked", worktree: "untracked", renamed_from: null }] : [],
          });
        if (changes && root?.endsWith(`/${fakeChange.file.name}`)) return reply({ path: root, size: fakeChange.file.content.length, truncated: false, encoding: "utf-8", content: fakeChange.file.content });
      }
      return missing();
    }
    if (collection === "agents") {
      if (id === "legacy") {
        if (req.headers.authorization !== "Bearer legacy-test") return res.writeHead(401).end();
        return reply({ name: "Imported reviewer", system: "Review the changes", skills: [], mcp_servers: {} });
      }
      if (!id && req.method === "POST" && b.metadata?.arugula_tab) return reply({ id: `tabagent${++state.agents}` });
      if (!id) return reply([]);
      return missing();
    }
    if (collection === "conversations") {
      if (!id && req.method === "POST" && b.workspace_id) {
        const conv = `tabconv${Object.keys(state.conversations).length + 1}`;
        state.conversations[conv] = { workspace: b.workspace_id, turns: b.prompt ? [{ prompt: b.prompt, answered: false }] : [], terminated: false, labels: b.labels };
        return reply({ id: conv, status: b.prompt ? "running" : "idle" });
      }
      if (!id) {
        const label = url.searchParams.get("label");
        return reply(Object.entries(state.conversations).filter(([, c]) => !label || `arugula_tab:${(c.labels as { arugula_tab?: string })?.arugula_tab}` === label).map(([id, c]) => ({ id, labels: c.labels })));
      }
      const c = state.conversations[id];
      if (!c) return missing();
      const r = rest.join("/");
      if (r === "events") return reply(events(c).filter((e) => (e.id as number) > Number(url.searchParams.get("after") ?? 0)));
      if (r.startsWith("requests/")) {
        const t = c.turns.at(-1);
        if (t) { t.answered = true; t.option = b.option_id; }
        return reply({});
      }
      if (r === "prompts") {
        if (waiting(c)) return reply({}, 409);
        c.turns.push({ prompt: b.prompt, answered: false });
        return reply({});
      }
      if (r === "interrupt") {
        if (!waiting(c)) return reply({}, 409);
        c.turns.at(-1)!.interrupted = true;
        return reply({});
      }
      if (r === "terminate") {
        c.terminated = true;
        return reply({});
      }
      if (r === "") return reply({ id, status: c.terminated ? "terminated" : waiting(c) ? "running" : "idle", usage_total: {} });
    }
    missing();
  }).listen(port, host);
  return { server, state: () => state };
}
