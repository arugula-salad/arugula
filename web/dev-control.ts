// `just dev-control [machine…]`: this checkout's web client as
// control.arugula.io serves it, signed in, with machines. A local
// arugula-control with a fake GitHub, daemons joined to it (relayed), and
// the web client rebuilt into the directory control serves on every change:
// edit, wait for "built", reload the page.
//
// A headless device of the account (fixtures/device.ts) approves the
// machines' joins and any browser that signs in, so the clicks are
// *Sign in with GitHub*, which signs in as $DEV_CONTROL_LOGIN (default
// $USER) with no GitHub, and (#761) *It matches* on the account's
// fingerprint: that device never sees the code the browser shows, so it
// approves the way a page from before codes does. Everything (control's
// database, the machines, that device) stays in the state directory
// between runs, and the port stays put, so the browser stays signed in.
//
// Control is at http://127.0.0.1:PORT, where passkeys are off (WebAuthn
// refuses an IP address); DEV_CONTROL_HOST=localhost puts it at
// http://localhost:PORT, with passkeys, in a state directory of its own.
// `web/dev-browser.ts` drives either from a headless Chrome, with a
// virtual passkey authenticator and a login of its choosing, for
// screenshots.
//
//   DEV_CONTROL_DIR    state [default: ~/.local/state/arugula-dev-control,
//                      or arugula-dev-control-HOST with DEV_CONTROL_HOST]
//   DEV_CONTROL_PORT   control's port [default: 7690]
//   DEV_CONTROL_HOST   control's host name [default: 127.0.0.1]; localhost
//                      for passkeys
//   DEV_CONTROL_LOGIN  who signs in [default: $USER]
//   DEV_CONTROL_LOGINS others the fake GitHub offers, comma-separated
//                      (sam,riley): it asks who on each sign-in. Their
//                      first browser is their account's first device;
//                      nothing approves their others.
//   DEV_CONTROL_LABS=0 machines without Labs (a stranger's view)
//   DEV_CONTROL_BLOCK_DOMAIN
//                      control's block domain (M50) [default:
//                      blocks.localhost, which Chrome sends to loopback;
//                      none with DEV_CONTROL_HOST=localhost]: ports and
//                      editors on the machines open in control's page,
//                      at http://b-<key>.<domain>:PORT. Empty for none.
//   DEV_CONTROL_APPROVE_BROWSERS=0
//                      the device leaves browsers alone: one that signs in
//                      stays waiting, not yet a device (for the waiting
//                      screens, *Add a machine and a device?* and #326).
//                      The machines' own joins are still approved.
//   DEV_CONTROL_CLOUD=1
//                      Cloud on (S40): a fake Managoat (fixtures/fakes.ts)
//                      and control's cloud flags, for every account. The
//                      first run makes three Cloud workspaces: app, ready
//                      with two tabs (one wrote a change, one waits on an
//                      approval), api, ready with settings saved since it
//                      was built (Rebuild needed), and web, whose setup
//                      failed (Retry sets it up). And a past run. The
//                      fake keeps its state beside control's database.
//   DEV_CONTROL_CLOUD_FULL=1
//                      with DEV_CONTROL_CLOUD, Managoat has no machine
//                      free for a rebuild (503 fleet_full): a Rebuild
//                      waits for a machine until it's cancelled.
//
// Start over: delete the state directory.

import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir, userInfo } from "node:os";
import { join } from "node:path";
import type { Server } from "node:http";
import { Device } from "./fixtures/device.ts";
import { fakeGithub, fakeManagoat, type FakeTurn } from "./fixtures/fakes.ts";
import { fingerprint, type Cert } from "./src/e2e/cert.ts";

// An agent's pane exports ARUGULA_* (the socket, keep-panes…) that would
// change what these daemons do (#682): not theirs.
for (const k of Object.keys(process.env)) if (/^(ARUGULA|ILLOGICAL)_/.test(k)) delete process.env[k];

const host = process.env.DEV_CONTROL_HOST || "127.0.0.1";
// What's joined to control (and the device) names it by its URL: a host
// of its own, a state directory of its own.
const dir = process.env.DEV_CONTROL_DIR ?? join(homedir(), ".local/state", host === "127.0.0.1" ? "arugula-dev-control" : `arugula-dev-control-${host}`);
const port = Number(process.env.DEV_CONTROL_PORT ?? 7690);
const login = process.env.DEV_CONTROL_LOGIN ?? userInfo().username;
// A block domain can't be a parent or child of control's host.
const blockDomain = process.env.DEV_CONTROL_BLOCK_DOMAIN ?? (host === "localhost" ? "" : "blocks.localhost");
const logins = (process.env.DEV_CONTROL_LOGINS ?? "").split(",").map((l) => l.trim()).filter(Boolean);
const labs = process.env.DEV_CONTROL_LABS !== "0";
const approveAll = process.env.DEV_CONTROL_APPROVE_BROWSERS !== "0";
const cloud = process.env.DEV_CONTROL_CLOUD === "1";
const machines = process.argv.slice(2).length ? process.argv.slice(2) : ["workstation", "laptop"];
const bin = process.env.TARGET_DIR ?? "../target/debug";
const base = `http://${host}:${port}`;
/** Where control listens: localhost may be ::1 first to a resolver, and
 * control listens on IPv4. */
const direct = `http://127.0.0.1:${port}`;
const via = { [base]: direct };
const web = join(dir, "web");

const procs: ChildProcess[] = [];
let github: Server | undefined;
let managoat: Server | undefined;
/** Each child leads a process group of its own (`detached`), and goes with
 * its whole group: `npx vite` runs vite under a shell, and a daemon its
 * panes' shims, which a signal to the child alone would leave running. */
const stop = (p: ChildProcess) => {
  try {
    if (p.pid) process.kill(-p.pid, "SIGTERM");
  } catch {
    p.kill("SIGTERM");
  }
};
const bye = (code = 0) => {
  for (const p of procs) stop(p);
  // Panes' shims outlive their daemon on purpose (they keep terminals
  // across a restart): anything still running from this state directory.
  try {
    execFileSync("pkill", ["-f", dir], { stdio: "ignore" });
  } catch {
    // none left, or no pkill
  }
  github?.close();
  managoat?.close();
  process.exit(code);
};
process.on("exit", () => procs.forEach(stop));
process.on("SIGINT", () => bye());
process.on("SIGTERM", () => bye());
const fail = (what: string) => {
  console.error(`dev-control: ${what}`);
  bye(1);
};
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const log = (s: string) => console.log(`  ${s}`);

mkdirSync(join(dir, "control"), { recursive: true });

// The web client, rebuilt on every change into what control serves. The
// service worker is built once beside it.
async function webBuild() {
  execFileSync("npx", ["vite", "build", "--logLevel", "error", "--config", "vite.sw.config.ts", "--outDir", web], { stdio: "inherit" });
  // Blocks' own scripts (M50), built once like the worker.
  for (const mode of ["boot", "sw", "shim"])
    execFileSync("npx", ["vite", "build", "--logLevel", "error", "--config", "vite.block.config.ts", "--mode", mode, "--outDir", join(web, "block")], { stdio: "inherit" });
  const watch = spawn("npx", ["vite", "build", "--watch", "--emptyOutDir", "false", "--outDir", web], { stdio: ["ignore", "pipe", "inherit"], detached: true });
  procs.push(watch);
  let first = true;
  await new Promise<void>((ready) => {
    watch.stdout!.on("data", (d: Buffer) => {
      if (!/built in/.test(d.toString())) return;
      if (first) {
        first = false;
        ready();
      } else log(`web rebuilt ${new Date().toLocaleTimeString()}: reload the page`);
    });
    watch.on("exit", (c) => fail(`vite build --watch exited (${c})`));
  });
}

async function up(url: string, proc: ChildProcess) {
  for (let i = 0; i < 200; i++) {
    if (proc.exitCode !== null) fail(`${url}: its process exited (${proc.exitCode})`);
    try {
      if ((await fetch(url)).status < 500) return;
    } catch {
      // not yet
    }
    await sleep(100);
  }
  fail(`${url} didn't come up`);
}

/** Nothing else on the port: an earlier dev-control's control would
 * answer the checks below, and this one would serve nothing. */
async function portFree() {
  const taken = await fetch(`${direct}/control.json`).then(
    () => true,
    () => false,
  );
  if (taken) fail(`${base} is in use (an earlier dev-control still running?). Stop it, or set DEV_CONTROL_PORT.`);
}

async function startControl() {
  github = fakeGithub(0, "127.0.0.1", () => login, logins);
  await new Promise((r) => github!.once("listening", r));
  const gh = `http://127.0.0.1:${(github.address() as { port: number }).port}`;
  const control = spawn(
    `${bin}/arugula-control`,
    [
      ...["--listen", `127.0.0.1:${port}`, "--public-url", base, "--db", join(dir, "control/control.db")],
      ...["--static-dir", web, "--github-client-id", "dev", "--github-client-secret", "dev"],
      ...["--github-url", gh, "--github-api", gh],
      ...(blockDomain ? ["--block-domain", blockDomain] : []),
      ...(cloud ? await cloudFlags() : []),
    ],
    { stdio: ["ignore", "ignore", "inherit"], detached: true },
  );
  procs.push(control);
  await up(`${direct}/control.json`, control);
}

/** Cloud's turns: what each tab's agent answers (fixtures/fakes.ts). */
function cloudTurn(prompt: string): FakeTurn {
  if (prompt === "Run the tests")
    return {
      asks: "pnpm test",
      blocks: [
        { kind: "thinking", body: "The suite is vitest; run it once and read the summary." },
        { kind: "text", body: "All 48 tests pass (6 files, 2.3 s). Nothing to fix." },
      ],
    };
  if (prompt === "Write the guide")
    return {
      writes: true,
      blocks: [
        { kind: "thinking", body: "README.md has no getting-started section; the guide belongs in docs/." },
        { kind: "text", body: "I added docs/guide.md with the first steps and linked it from README.md. Open Changes to read the diff." },
      ],
    };
  return { blocks: [{ kind: "text", body: `(the fake Managoat) Done: ${prompt}` }] };
}

/** Cloud on: a fake Managoat that keeps its state beside control's
 * database, and control's flags for it. The secrets key is made on the
 * first run and kept, so saved secrets open on the next. */
async function cloudFlags(): Promise<string[]> {
  const m = fakeManagoat(0, "127.0.0.1", {
    file: join(dir, "control/managoat.json"),
    // The setup fails until a retry: a Cloud workspace whose script runs pnpm.
    fails: (env, setup) => (setup === 1 && /\bpnpm\b/.test(String(env.setup_script ?? "")) ? "setup_script exited 127: pnpm: command not found" : null),
    turn: cloudTurn,
    full: () => process.env.DEV_CONTROL_CLOUD_FULL === "1",
  });
  managoat = m.server;
  if (!managoat.listening) await new Promise((r) => managoat!.once("listening", r));
  const keyFile = join(dir, "control/cloud-secret-key");
  if (!existsSync(keyFile)) writeFileSync(keyFile, randomBytes(32).toString("hex"), { mode: 0o600 });
  return [
    ...["--cloud-provider-url", `http://127.0.0.1:${(managoat.address() as { port: number }).port}`, "--cloud-provider-key", "dev"],
    ...["--cloud-secret-key", readFileSync(keyFile, "utf8").trim(), "--cloud-accounts", "*"],
    ...["--cloud-models", "claude:anthropic/claude-sonnet-4-5,codex:openai/gpt-5-codex"],
  ];
}

interface DevCloudWorkspace {
  id: string;
  status: string;
  saved_spec: Record<string, unknown>;
}

/** The first run with Cloud on: three Cloud workspaces to look at, made
 * through control as the account (DEV_CONTROL_CLOUD above), and a past
 * run. Later runs find them there. */
async function seedCloud(d: Device) {
  const { cloud_workspaces } = await d.api<{ cloud_workspaces: DevCloudWorkspace[] }>("/api/cloud/workspaces");
  if (cloud_workspaces.length) return;
  log("making Cloud workspaces: app, api, web…");
  const create = (name: string, spec: Record<string, unknown>) =>
    d.api<DevCloudWorkspace>("/api/cloud/workspaces", { request_key: `dev-${name}`, spec: { name, repositories: [{ url: `https://github.com/example/${name}` }], ...spec } });
  const app = await create("app", { packages: { apt: ["ffmpeg"] }, setup_script: "npm ci" });
  const api = await create("api", { packages: { apt: ["postgresql-client"] }, setup_script: "cargo fetch" });
  await create("web", { setup_script: "pnpm install" });
  const settled = async (w: DevCloudWorkspace) => {
    for (let i = 0; i < 300; i++) {
      const now = await d.api<DevCloudWorkspace>(`/api/cloud/workspaces/${w.id}`);
      if (now.status !== "provisioning") return now;
      await sleep(500);
    }
    throw new Error(`Cloud workspace ${w.id} didn't set up`);
  };
  const tab = (w: DevCloudWorkspace, key: string, agent: string, prompt: string) =>
    d.api(`/api/cloud/workspaces/${w.id}/tabs`, { request_key: key, agent_id: agent, runtime: "claude", model: "anthropic/claude-sonnet-4-5", prompt });
  await tab(await settled(app), "dev-app-guide", "curated_coding", "Write the guide");
  await tab(app, "dev-app-tests", "curated_review", "Run the tests");
  const built = await settled(api);
  await tab(built, "dev-api-docs", "curated_coding", "Explain the API's routes");
  // Saved after it was built: what a rebuild would change.
  await d.api(`/api/cloud/workspaces/${api.id}`, { spec: { ...built.saved_spec, packages: { apt: ["postgresql-client", "jq"] }, env_vars: { RUST_LOG: "debug" } } }, "PATCH");
  // A run from before runs were retired (M119), as control kept them.
  const { account } = await d.api<{ account: string }>("/api/me");
  const { DatabaseSync } = await import("node:sqlite");
  const db = new DatabaseSync(join(dir, "control/control.db"));
  db.exec("PRAGMA busy_timeout=5000");
  const run = `run_${randomBytes(16).toString("hex")}`;
  const created = Date.now() - 3 * 86_400_000;
  const spec = {
    agent_id: "curated_coding", agent_version: 1,
    agent: { name: "Coding agent", instructions: "Implement the requested change.", mcp_servers: {}, skills: [], secret_names: [] },
    model: "anthropic/claude-sonnet-4-5", runtime: "claude", repository: "https://github.com/example/app", git_ref: "", prompt: "Fix the flaky upload test",
  };
  const result = { diff: { diff: "diff --git a/test/upload.test.ts b/test/upload.test.ts\n--- a/test/upload.test.ts\n+++ b/test/upload.test.ts\n@@ -3 +3 @@\n-  await sleep(100);\n+  await upload.done;\n" }, files: {} };
  db.prepare("INSERT INTO cloud_runs(id,account,request_key,spec,state,created,deadline,result) VALUES(?,?,?,?,?,?,?,?)").run(run, account, run, JSON.stringify(spec), "completed", created, created + 1_800_000, JSON.stringify(result));
  // The page shows the run's prompt itself.
  const events = [{ id: 1, blocks: [{ kind: "text", body: "The test slept instead of waiting for the upload; it waits on the upload now." }] }];
  for (const e of events) db.prepare("INSERT INTO cloud_events(conversation,account,cursor,body) VALUES(?,?,?,?)").run(run, account, e.id, JSON.stringify(e));
  db.close();
  log("Cloud workspaces made");
}

/** The account's headless device: made on the first run, kept after. */
async function device(): Promise<Device> {
  const file = join(dir, "device.json");
  let d: Device;
  if (existsSync(file)) {
    d = await Device.load(file);
    await d.session(login);
  } else d = await Device.signIn({ control: base, via, login, name: "dev-control" });
  await d.save(file);
  return d;
}

/** `arugulad join`, approved by the device. */
async function join_(d: Device, name: string, state: string) {
  const joining = spawn(`${bin}/arugulad`, ["join", base, "--name", name, "--state-dir", state], { stdio: ["pipe", "pipe", "inherit"] });
  let out = "";
  const code = await new Promise<string>((res) => {
    joining.stdout!.on("data", (b: Buffer) => {
      out += b;
      // The whole code: the machine's secret follows it (#760).
      const m = out.match(/#join=([A-Z0-9.-]+)\s/);
      if (m) res(m[1]);
    });
    joining.on("exit", (c) => fail(`joining ${name} exited (${c}):\n${out}`));
  });
  joining.removeAllListeners("exit");
  const exited = new Promise<number | null>((r) => joining.on("exit", r));
  // Its answer to the secret is all the machine needs: it asks nothing.
  await d.approveJoin(code);
  if ((await exited) !== 0) fail(`joining ${name} failed:\n${out}`);
}

async function startMachines(d: Device) {
  const listed = new Set((await d.directory()).map((m) => m.name));
  for (const name of machines) {
    const state = join(dir, "machines", name);
    mkdirSync(state, { recursive: true });
    if (!listed.has(name)) {
      await join_(d, name, state);
      log(`${name} joined`);
    }
    const flags = join(state, "flags.json");
    if (labs && !existsSync(flags)) writeFileSync(flags, JSON.stringify({ flags: { labs: true } }));
    procs.push(
      spawn(
        `${bin}/arugulad`,
        [...["--listen", "127.0.0.1:0", "--name", name, "--state-dir", state], "--no-manager-env", ...["--tailscale-socket", "/nonexistent/sock"]],
        { stdio: "ignore", detached: true },
      ),
    );
  }
  for (const name of machines) {
    const m = (await d.directory()).find((x) => x.name === name);
    if (m) await d.waitOnline(m.id).catch(() => log(`${name} isn't online yet`));
  }
}

/** Any browser that signs in is approved, as another device of yours would. */
async function approveBrowsers(d: Device) {
  for (;;) {
    try {
      const { pending } = await d.api<{ pending: Cert[] }>("/api/devices");
      for (const c of pending) {
        // #761: without the code it shows (only its screen has that), so
        // the browser asks to check the account's fingerprint.
        await d.approveRequest(c);
        log(`approved ${c.name || "a browser"}: check the account is ${fingerprint(d.root)} and press It matches`);
      }
    } catch (e) {
      // Control restarting, or the session gone: sign in again.
      await d.session(login).catch(() => {});
      if (!(e instanceof Error && /401|403/.test(e.message))) log(`approving: ${String(e)}`);
    }
    await sleep(1000);
  }
}

await portFree();
console.log("dev-control: building the web client…");
await webBuild();
await startControl();
const d = await device();
await startMachines(d);
if (cloud) await seedCloud(d).catch((e) => log(`Cloud workspaces: ${String(e)}`));
console.log(`
dev-control: ${base}${host === "127.0.0.1" ? " (no passkeys on an IP address: DEV_CONTROL_HOST=localhost)" : ""}
  Sign in with GitHub (the fake one signs you in as ${login}${logins.length ? `, or asks: ${[login, ...logins].join(", ")}` : ""});
  ${approveAll ? `${login}'s browsers are approved (press It matches: the account is ${fingerprint(d.root)})` : `${login}'s browsers stay waiting to be approved (DEV_CONTROL_APPROVE_BROWSERS=0)`}. Machines: ${machines.join(", ")}${labs ? ", with Labs" : ""}.
  ${blockDomain ? `Ports and editors open on b-<key>.${blockDomain} (Chrome; DEV_CONTROL_BLOCK_DOMAIN= for none; the block scripts aren't rebuilt on change).` : "No block domain: ports and editors are off on the machines."}
  ${cloud ? "Cloud is on, with a fake Managoat: Cloud workspaces app, api and web, and a past run." : "No Cloud (DEV_CONTROL_CLOUD=1 for Cloud workspaces with a fake Managoat)."}
  Edit web/src, wait for "web rebuilt", reload. State: ${dir}. Ctrl-C stops.
`);
// Leaving browsers alone: stay up for the web build and the machines.
if (approveAll) await approveBrowsers(d);
else setInterval(() => {}, 1 << 30);
