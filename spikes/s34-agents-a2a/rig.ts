// S34 rig: a local control (fake GitHub), two people ("jake" and "ale") in
// one team, and three machines: jake-box and ale-box in the team, ale-solo
// ale's alone. Each person's CLI is logged in with its own config dir. ale
// offers `fixer` (a Claude Code subagent file in a small repo) on ale-box
// and ale-solo; jake has a clone of the same repo.
//
//   node --experimental-strip-types --no-warnings spikes/s34-agents-a2a/rig.ts STATE_DIR
//
// It writes STATE_DIR/rig.json (and env files to source per person), then
// stays up until interrupted. act.ts answers cards as a person's laptop.

import { spawn, execFileSync, type ChildProcess } from "node:child_process";
import { createServer as createTcp } from "node:net";
import { mkdirSync, writeFileSync, rmSync } from "node:fs";
import { join, resolve } from "node:path";
import { joinCode } from "../../web/src/e2e/cert.ts";
import { Device } from "../../web/fixtures/device.ts";
import { fakeGithub } from "../../web/fixtures/fakes.ts";

const root = resolve(process.argv[2] ?? "/tmp/s34-rig");
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
const [CONTROL, GITHUB, JAKE_BOX, ALE_BOX, ALE_SOLO] = await Promise.all(Array.from({ length: 5 }, freePort));
const base = `http://127.0.0.1:${CONTROL}`;
const procs: ChildProcess[] = [];
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const dir = (...p: string[]) => {
  const d = join(root, ...p);
  mkdirSync(d, { recursive: true });
  return d;
};

// The outer session's variables would point daemons and CLIs at the
// user's own daemon (see e2e-local-env-gotchas).
const clean: Record<string, string> = {};
for (const [k, v] of Object.entries(process.env)) {
  if (v !== undefined && !/^(CLAUDE|ILLOGICAL|AI_AGENT)/.test(k)) clean[k] = v;
}
clean.PATH = `${target}:${clean.PATH}`;
const envFor = (who: string) => ({ ...clean, XDG_CONFIG_HOME: dir(who, "config"), ILLOGICAL_SOCK: join(root, who, "no-daemon.sock") });

const gh = fakeGithub(GITHUB);
const shutdown = () => {
  for (const p of procs) p.kill("SIGTERM");
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
  spawn(`${target}/illogical-control`, [
    ...["--listen", `127.0.0.1:${CONTROL}`, "--public-url", base, "--db", join(dir("control"), "control.db")],
    ...["--github-client-id", "id", "--github-client-secret", "secret"],
    ...["--github-url", `http://127.0.0.1:${GITHUB}`, "--github-api", `http://127.0.0.1:${GITHUB}`],
    ...["--relay-free-mb", "1000"],
  ], { stdio: ["ignore", "ignore", "inherit"], env: { ...clean, RUST_LOG: "illogical_control=warn" } }),
);
await up(`${base}/control.json`);

// People, and a team.
const jake = await Device.signIn({ control: base, login: "jake", name: "jake's laptop" });
const ale = await Device.signIn({ control: base, login: "ale", name: "ale's laptop" });
const team = await jake.createTeam("crew");
await ale.acceptInvite(team, await jake.invite(team, "editor"));
await jake.admitAll(team);
await jake.save(join(root, "jake.device.json"));
await ale.save(join(root, "ale.device.json"));

// Machines.
async function machine(name: string, by: Device, port: number, inTeam: boolean, who: string) {
  const state = dir(who, name);
  const joining = spawn(`${target}/illogicald`, ["join", base, "--name", name, "--state-dir", state, "--account", by.keys.id], {
    stdio: ["ignore", "pipe", "inherit"],
    env: envFor(who),
  });
  const code = await new Promise<string>((res) => {
    let out = "";
    joining.stdout!.on("data", (d) => {
      out += d;
      const m = out.match(/#join=([A-Z0-9]{5}-[A-Z0-9]{5})/);
      if (m) res(m[1]);
    });
  });
  const c = await by.approveJoin(code, inTeam ? team : undefined);
  if ((await joinCode(c)) !== code) throw new Error("join code mismatch");
  await new Promise((r) => joining.on("exit", r));
  procs.push(
    spawn(`${target}/illogicald`, [
      ...["--listen", `127.0.0.1:${port}`, "--name", name, "--state-dir", state],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent", "--no-claude-ide"],
    ], { stdio: ["ignore", "ignore", "pipe"], env: { ...envFor(who), RUST_LOG: "illogicald=info" } }),
  );
  const log: string[] = [];
  procs.at(-1)!.stderr!.on("data", (d: Buffer) => {
    const s = d.toString();
    if (/a2a|A2A/.test(s)) process.stdout.write(`[${name}] ${s}`);
    log.push(s);
  });
  const listed = await by.waitOnline(name, 20_000);
  return { name, id: listed.id, state, port, url: `http://127.0.0.1:${port}` };
}
const jakeBox = await machine("jake-box", jake, JAKE_BOX, true, "jake");
const aleBox = await machine("ale-box", ale, ALE_BOX, true, "ale");
const aleSolo = await machine("ale-solo", ale, ALE_SOLO, false, "ale");

// CLIs, logged in as each person.
async function cliLogin(who: string, by: Device) {
  const login = spawn(`${target}/illogical`, ["login", base, "--name", `${who}'s cli`, "--account", by.keys.id], {
    env: envFor(who),
    stdio: ["ignore", "pipe", "inherit"],
  });
  let said = "";
  const code = await new Promise<string>((res) => {
    login.stdout!.on("data", (d) => {
      said += d;
      const m = said.match(/#join=([A-Z0-9]{5}-[A-Z0-9]{5})/);
      if (m) res(m[1]);
    });
  });
  await by.approveJoin(code);
  const exit = await new Promise<number>((r) => login.on("exit", (c) => r(c ?? 1)));
  if (exit !== 0) throw new Error(`${who}'s login: ${said}`);
}
await cliLogin("jake", jake);
await cliLogin("ale", ale);

// ale's project, with an agent offered; jake's clone of it.
const repo = dir("ale", "calc");
const git = (cwd: string, ...a: string[]) => execFileSync("git", ["-C", cwd, ...a], { env: clean }).toString();
git(repo, "init", "-q", "-b", "main");
writeFileSync(
  join(repo, "calc.py"),
  `def mean(xs):\n    """The arithmetic mean of a non-empty list."""\n    return sum(xs) / (len(xs) - 1)\n\n\ndef median(xs):\n    s = sorted(xs)\n    return s[len(s) // 2]\n`,
);
writeFileSync(join(repo, "test_calc.py"), `from calc import mean, median\n\n\ndef test_mean():\n    assert mean([1, 2, 3]) == 2\n\n\ndef test_median_even():\n    assert median([1, 2, 3, 4]) == 2.5\n`);
mkdirSync(join(repo, ".claude/agents"), { recursive: true });
writeFileSync(
  join(repo, ".claude/agents/fixer.md"),
  `---
name: fixer
description: Fixes failing Python tests in this repository with the smallest change, and says what it changed.
tools: Read, Edit, Write, Grep, Glob, Bash
model: haiku
---
You fix bugs. Run the tests with \`python3 -m pytest -q\` (or \`python3 -c\` checks if pytest is missing),
find the cause, make the smallest change that fixes it, and run the tests again. Don't commit.
End with two or three sentences: what was wrong, and what you changed.
`,
);
git(repo, "add", "-A");
git(repo, "-c", "user.name=ale", "-c", "user.email=ale@example.com", "commit", "-q", "-m", "calc");
const clone = join(dir("jake"), "calc");
execFileSync("git", ["clone", "-q", repo, clone], { env: clean });

const rig = {
  control: base,
  team,
  people: {
    jake: { account: jake.account, config: join(root, "jake", "config"), device: join(root, "jake.device.json") },
    ale: { account: ale.account, config: join(root, "ale", "config"), device: join(root, "ale.device.json") },
  },
  machines: { "jake-box": jakeBox, "ale-box": aleBox, "ale-solo": aleSolo },
  repo,
  clone,
  target,
};
writeFileSync(join(root, "rig.json"), JSON.stringify(rig, null, 2));
for (const who of ["jake", "ale"]) {
  const e = envFor(who);
  writeFileSync(
    join(root, `${who}.env`),
    `export PATH='${e.PATH}' XDG_CONFIG_HOME='${e.XDG_CONFIG_HOME}' ILLOGICAL_SOCK='${e.ILLOGICAL_SOCK}'\nunset CLAUDECODE AI_AGENT ILLOGICAL_PANE\n`,
  );
}
console.log(`rig up: ${root}/rig.json`);
await new Promise(() => {});
