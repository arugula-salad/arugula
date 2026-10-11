// `just dev-control [machine…]`: this checkout's web client as
// control.arugula.io serves it, signed in, with machines. A local
// arugula-control with a fake GitHub, daemons joined to it (relayed), and
// the web client rebuilt into the directory control serves on every change:
// edit, wait for "built", reload the page.
//
// A headless device of the account (fixtures/device.ts) approves the
// machines' joins and any browser that signs in, so the only click is
// *Sign in with GitHub*, which signs in as $DEV_CONTROL_LOGIN (default
// $USER) with no GitHub. Everything (control's database, the machines,
// that device) stays in the state directory between runs, and the port
// stays put, so the browser stays signed in.
//
//   DEV_CONTROL_DIR    state [default: ~/.local/state/arugula-dev-control]
//   DEV_CONTROL_PORT   control's port [default: 7690]
//   DEV_CONTROL_LOGIN  who signs in [default: $USER]
//   DEV_CONTROL_LABS=0 machines without Labs (a stranger's view)
//
// Start over: delete the state directory.

import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { homedir, userInfo } from "node:os";
import { join } from "node:path";
import type { Server } from "node:http";
import { Device } from "./fixtures/device.ts";
import { fakeGithub } from "./fixtures/fakes.ts";
import type { Cert } from "./src/e2e/cert.ts";

// An agent's pane exports ARUGULA_* (the socket, keep-panes…) that would
// change what these daemons do (#682): not theirs.
for (const k of Object.keys(process.env)) if (/^(ARUGULA|ILLOGICAL)_/.test(k)) delete process.env[k];

const dir = process.env.DEV_CONTROL_DIR ?? join(homedir(), ".local/state/arugula-dev-control");
const port = Number(process.env.DEV_CONTROL_PORT ?? 7690);
const login = process.env.DEV_CONTROL_LOGIN ?? userInfo().username;
const labs = process.env.DEV_CONTROL_LABS !== "0";
const machines = process.argv.slice(2).length ? process.argv.slice(2) : ["workstation", "laptop"];
const bin = process.env.TARGET_DIR ?? "../target/debug";
const base = `http://127.0.0.1:${port}`;
const web = join(dir, "web");

const procs: ChildProcess[] = [];
let github: Server | undefined;
const bye = (code = 0) => {
  for (const p of procs) p.kill("SIGTERM");
  github?.close();
  process.exit(code);
};
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
  const watch = spawn("npx", ["vite", "build", "--watch", "--emptyOutDir", "false", "--outDir", web], { stdio: ["ignore", "pipe", "inherit"] });
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

async function startControl() {
  github = fakeGithub(0, "127.0.0.1", () => login);
  await new Promise((r) => github!.once("listening", r));
  const gh = `http://127.0.0.1:${(github.address() as { port: number }).port}`;
  const control = spawn(
    `${bin}/arugula-control`,
    [
      ...["--listen", `127.0.0.1:${port}`, "--public-url", base, "--db", join(dir, "control/control.db")],
      ...["--static-dir", web, "--github-client-id", "dev", "--github-client-secret", "dev"],
      ...["--github-url", gh, "--github-api", gh],
    ],
    { stdio: ["ignore", "ignore", "inherit"] },
  );
  procs.push(control);
  await up(`${base}/control.json`, control);
}

/** The account's headless device: made on the first run, kept after. */
async function device(): Promise<Device> {
  const file = join(dir, "device.json");
  let d: Device;
  if (existsSync(file)) {
    d = await Device.load(file);
    await d.session(login);
  } else d = await Device.signIn({ control: base, login, name: "dev-control" });
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
      const m = out.match(/#join=([A-Z0-9-]+)/);
      if (m) res(m[1]);
    });
    joining.on("exit", (c) => fail(`joining ${name} exited (${c}):\n${out}`));
  });
  joining.removeAllListeners("exit");
  const exited = new Promise<number | null>((r) => joining.on("exit", r));
  await d.approveJoin(code);
  // The fingerprint it asks about is the one the device approved with.
  joining.stdin!.end("y\n");
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
        { stdio: "ignore" },
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
        await d.approveRequest(c);
        log(`approved ${c.name || "a browser"}`);
      }
    } catch (e) {
      // Control restarting, or the session gone: sign in again.
      await d.session(login).catch(() => {});
      if (!(e instanceof Error && /401|403/.test(e.message))) log(`approving: ${String(e)}`);
    }
    await sleep(1000);
  }
}

console.log("dev-control: building the web client…");
await webBuild();
await startControl();
const d = await device();
await startMachines(d);
console.log(`
dev-control: ${base}
  Sign in with GitHub (the fake one signs you in as ${login}); this browser
  is approved for you. Machines: ${machines.join(", ")}${labs ? ", with Labs" : ""}.
  Edit web/src, wait for "web rebuilt", reload. State: ${dir}. Ctrl-C stops.
`);
await approveBrowsers(d);
