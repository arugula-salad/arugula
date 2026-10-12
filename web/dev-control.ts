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
//
// Start over: delete the state directory.

import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { homedir, userInfo } from "node:os";
import { join } from "node:path";
import type { Server } from "node:http";
import { Device } from "./fixtures/device.ts";
import { fakeGithub } from "./fixtures/fakes.ts";
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
    ],
    { stdio: ["ignore", "ignore", "inherit"], detached: true },
  );
  procs.push(control);
  await up(`${direct}/control.json`, control);
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
console.log(`
dev-control: ${base}${host === "127.0.0.1" ? " (no passkeys on an IP address: DEV_CONTROL_HOST=localhost)" : ""}
  Sign in with GitHub (the fake one signs you in as ${login}${logins.length ? `, or asks: ${[login, ...logins].join(", ")}` : ""});
  ${approveAll ? `${login}'s browsers are approved (press It matches: the account is ${fingerprint(d.root)})` : `${login}'s browsers stay waiting to be approved (DEV_CONTROL_APPROVE_BROWSERS=0)`}. Machines: ${machines.join(", ")}${labs ? ", with Labs" : ""}.
  ${blockDomain ? `Ports and editors open on b-<key>.${blockDomain} (Chrome; DEV_CONTROL_BLOCK_DOMAIN= for none; the block scripts aren't rebuilt on change).` : "No block domain: ports and editors are off on the machines."}
  Edit web/src, wait for "web rebuilt", reload. State: ${dir}. Ctrl-C stops.
`);
// Leaving browsers alone: stay up for the web build and the machines.
if (approveAll) await approveBrowsers(d);
else setInterval(() => {}, 1 << 30);
