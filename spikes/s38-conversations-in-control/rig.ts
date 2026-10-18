// S38 rig (S36's, with leads on haiku): a local control (fake GitHub), one
// person ("jake"), and two of his machines, box-a (where the conversation
// is made, its first lead) and box-b, both relayed through control as two
// machines of one account are. Labs is on.
//
//   node --experimental-strip-types --no-warnings spikes/s38-conversations-in-control/rig.ts STATE_DIR
//
// It writes STATE_DIR/rig.json (each machine's id, port, state dir and
// local token) and stays up until interrupted. Nothing here reads or
// writes the user's own daemon: the environment is stripped of ARUGULA_*
// and CLAUDE*, each daemon has its own state dir (and so its own socket).

import { spawn, execFileSync, type ChildProcess } from "node:child_process";
import { createServer as createTcp } from "node:net";
import { mkdirSync, writeFileSync, readFileSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { Device } from "../../web/fixtures/device.ts";
import { fakeGithub } from "../../web/fixtures/fakes.ts";

const root = resolve(process.argv[2] ?? "/tmp/s38-rig");
rmSync(root, { recursive: true, force: true });
mkdirSync(root, { recursive: true });
const target = resolve(process.env.TARGET_DIR ?? "target/debug");

async function freePort(): Promise<number> {
  const s = createTcp().listen(0, "127.0.0.1");
  await new Promise((ok) => s.once("listening", ok));
  const port = (s.address() as { port: number }).port;
  await new Promise((ok) => s.close(ok));
  return port;
}
const [CONTROL, GITHUB, BOX_A, BOX_B] = await Promise.all(Array.from({ length: 4 }, freePort));
const base = `http://127.0.0.1:${CONTROL}`;
const procs: ChildProcess[] = [];
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const dir = (...p: string[]) => {
  const d = join(root, ...p);
  mkdirSync(d, { recursive: true });
  return d;
};

const clean: Record<string, string> = {};
for (const [k, v] of Object.entries(process.env)) {
  if (v !== undefined && !/^(CLAUDE|ARUGULA|ILLOGICAL|AI_AGENT)/.test(k)) clean[k] = v;
}
clean.PATH = `${target}:${clean.PATH}`;
// The leads run on haiku (the prototype reads this in `lead::user_model`).
clean.S38_LEAD_MODEL = "haiku";

const gh = fakeGithub(GITHUB, "127.0.0.1", () => "jake");
const shutdown = () => {
  for (const p of procs) {
    try {
      if (p.pid) process.kill(-p.pid, "SIGTERM");
    } catch {
      p.kill("SIGTERM");
    }
  }
  gh.close();
  process.exit(0);
};
process.on("SIGINT", shutdown);
process.on("SIGTERM", shutdown);

async function up(url: string) {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(url)).status < 500) return;
    } catch {
      // not yet
    }
    await sleep(100);
  }
  throw new Error(`${url} didn't come up`);
}

procs.push(
  spawn(
    `${target}/arugula-control`,
    [
      ...["--listen", `127.0.0.1:${CONTROL}`, "--public-url", base, "--db", join(dir("control"), "control.db")],
      ...["--github-client-id", "id", "--github-client-secret", "secret"],
      ...["--github-url", `http://127.0.0.1:${GITHUB}`, "--github-api", `http://127.0.0.1:${GITHUB}`],
    ],
    { stdio: ["ignore", "ignore", "inherit"], env: { ...clean, RUST_LOG: "arugula_control=warn" }, detached: true },
  ),
);
await up(`${base}/control.json`);

const jake = await Device.signIn({ control: base, login: "jake", name: "jake's laptop" });
await jake.save(join(root, "jake.device.json"));

async function machine(name: string, port: number) {
  const state = dir(name);
  writeFileSync(join(state, "flags.json"), JSON.stringify({ flags: { labs: true } }));
  const joining = spawn(`${target}/arugulad`, ["join", base, "--name", name, "--state-dir", state], {
    stdio: ["pipe", "pipe", "inherit"],
    env: clean,
  });
  let out = "";
  const code = await new Promise<string>((res) => {
    joining.stdout!.on("data", (d) => {
      out += d;
      const m = out.match(/#join=([A-Z0-9.-]+)\s/);
      if (m) res(m[1]);
    });
  });
  await jake.approveJoin(code);
  await new Promise((r) => joining.on("exit", r));
  const log = join(root, `${name}.log`);
  const p = spawn(
    `${target}/arugulad`,
    [
      ...["--listen", `127.0.0.1:${port}`, "--name", name, "--state-dir", state],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent", "--no-claude-ide"],
      // Its log: tracing writes to stdout, which this rig doesn't keep.
      ...["--log-file", log],
    ],
    { stdio: ["ignore", "ignore", "pipe"], env: { ...clean, RUST_LOG: "arugulad=info" }, detached: true },
  );
  procs.push(p);
  const lines: string[] = [];
  p.stderr!.on("data", (d: Buffer) => {
    lines.push(d.toString());
    writeFileSync(log, lines.join(""));
  });
  const listed = await jake.waitOnline(name, 30_000);
  await up(`http://127.0.0.1:${port}/`);
  return { name, id: listed.id, pid: p.pid, state, port, url: `http://127.0.0.1:${port}`, token: readFileSync(join(state, "local-token"), "utf8").trim() };
}
const a = await machine("box-a", BOX_A);
const b = await machine("box-b", BOX_B);

// A small project on each machine, for the agents there.
for (const m of ["box-a", "box-b"]) {
  const repo = dir(m, "work", "calc");
  const git = (...args: string[]) => execFileSync("git", ["-C", repo, ...args], { env: clean });
  git("init", "-q", "-b", "main");
  writeFileSync(join(repo, "calc.py"), `def mean(xs):\n    return sum(xs) / (len(xs) - 1)\n`);
  writeFileSync(join(repo, "test_calc.py"), `from calc import mean\n\n\ndef test_mean():\n    assert mean([1, 2, 3]) == 2\n`);
  git("add", "-A");
  git("-c", "user.name=jake", "-c", "user.email=jake@example.com", "commit", "-q", "-m", "calc");
}

writeFileSync(join(root, "rig.json"), JSON.stringify({ control: base, machines: { "box-a": a, "box-b": b }, target }, null, 2));
console.log(`rig up: ${root}/rig.json`);
await new Promise(() => {});
