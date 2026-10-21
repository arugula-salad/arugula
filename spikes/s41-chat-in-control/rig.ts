// S41 rig: chat held by control, in plain text, with every machine off.
//
//   node --experimental-strip-types --no-warnings spikes/s41-chat-in-control/rig.ts STATE_DIR
//
// One local arugula-control (fake GitHub, a fake Web Push service), two
// people (jake and alex, each a headless browser device from
// web/fixtures/device.ts) in one team, and two machines: box-a (jake's, in
// the team) and box-b (alex's own, not in the team). It:
//
//  1. runs a pane on each machine and starts an M61 thread on box-a's pane
//     the way it's done today (on the daemon);
//  2. has jake and alex each capture a pane over the relay (end to end, as
//     the page does before quoting);
//  3. stops both daemons by PID;
//  4. imports box-a's old threads into control from its state dir;
//  5. with both machines off: a channel, a DM, a quoted pane on each side,
//     a mention push to alex's phone (decrypted here), live delivery over
//     the SSE stream, unread counts, read marks and search;
//  6. writes STATE_DIR/flow.txt and leaves control's database for dump.py.
//
// Nothing here touches the user's own daemon: the environment is stripped
// of ARUGULA_*/CLAUDE*, each daemon has its own state dir and port, and
// every process is stopped by PID.

import { spawn, type ChildProcess } from "node:child_process";
import { createECDH, createDecipheriv, hkdfSync, randomBytes } from "node:crypto";
import { createServer as createTcp } from "node:net";
import { mkdirSync, writeFileSync, readFileSync, readdirSync, rmSync, appendFileSync, existsSync } from "node:fs";
import { join, resolve } from "node:path";
import { Device } from "../../web/fixtures/device.ts";
import { fakeGithub, fakePush } from "../../web/fixtures/fakes.ts";
import { signText } from "../../web/src/shell/e2e/keys.ts";

const root = resolve(process.argv[2] ?? "/tmp/s41-rig");
rmSync(root, { recursive: true, force: true });
mkdirSync(root, { recursive: true });
const target = resolve(process.env.TARGET_DIR ?? "target/debug");
const flow = join(root, "flow.txt");
const t0 = Date.now();
const say = (...a: unknown[]) => {
  const line = `[${((Date.now() - t0) / 1000).toFixed(1).padStart(5)}s] ${a.map((x) => (typeof x === "string" ? x : JSON.stringify(x))).join(" ")}`;
  console.log(line);
  appendFileSync(flow, line + "\n");
};

async function freePort(): Promise<number> {
  const s = createTcp().listen(0, "127.0.0.1");
  await new Promise((ok) => s.once("listening", ok));
  const port = (s.address() as { port: number }).port;
  await new Promise((ok) => s.close(ok));
  return port;
}
const [CONTROL, GITHUB, PUSH, BOX_A, BOX_B] = await Promise.all(Array.from({ length: 5 }, freePort));
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

const gh = fakeGithub(GITHUB, "127.0.0.1", () => "jake");
const push = fakePush(PUSH);
const stopAll = () => {
  for (const p of procs) {
    try {
      if (p.pid && p.exitCode === null) process.kill(-p.pid, "SIGTERM");
    } catch {
      // gone
    }
  }
  gh.close();
  push.server.close();
};
process.on("SIGINT", () => (stopAll(), process.exit(1)));
process.on("SIGTERM", () => (stopAll(), process.exit(1)));

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

const control = spawn(
  `${target}/arugula-control`,
  [
    ...["--listen", `127.0.0.1:${CONTROL}`, "--public-url", base, "--db", join(dir("control"), "control.db")],
    ...["--github-client-id", "id", "--github-client-secret", "secret"],
    ...["--github-url", `http://127.0.0.1:${GITHUB}`, "--github-api", `http://127.0.0.1:${GITHUB}`],
    ...["--push-host", `127.0.0.1:${PUSH}`],
  ],
  { stdio: ["ignore", "ignore", "inherit"], env: { ...clean, RUST_LOG: "arugula_control=warn" }, detached: true },
);
procs.push(control);
await up(`${base}/control.json`);
say(`control up at ${base} (pid ${control.pid})`);

// ---- two people, one team
const jake = await Device.signIn({ control: base, login: "jake", name: "jake's laptop" });
const alex = await Device.signIn({ control: base, login: "alex", name: "alex's phone", kind: "browser" });
const team = await jake.createTeam("acme");
await alex.acceptInvite(team, await jake.invite(team, "editor"));
await jake.admitAll(team);
say(`team acme (${team}): jake owner, alex editor`);

// ---- two machines
async function machine(name: string, port: number, owner: Device, inTeam?: string) {
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
  await owner.approveJoin(code, inTeam);
  await new Promise((r) => joining.on("exit", r));
  const log = join(root, `${name}.log`);
  const p = spawn(
    `${target}/arugulad`,
    [
      ...["--listen", `127.0.0.1:${port}`, "--name", name, "--state-dir", state],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent", "--no-claude-ide"],
      ...["--log-file", log],
    ],
    { stdio: ["ignore", "ignore", "ignore"], env: { ...clean, RUST_LOG: "arugulad=info" }, detached: true },
  );
  procs.push(p);
  const listed = await owner.waitOnline(name, 30_000);
  await up(`http://127.0.0.1:${port}/`);
  const token = readFileSync(join(state, "local-token"), "utf8").trim();
  const local = async <T>(method: string, path: string, body?: unknown): Promise<T> => {
    const r = await fetch(`http://127.0.0.1:${port}${path}`, {
      method,
      headers: { authorization: `Bearer ${token}`, ...(body ? { "content-type": "application/json" } : {}) },
      body: body ? JSON.stringify(body) : undefined,
    });
    const text = await r.text();
    if (!r.ok) throw new Error(`${name} ${method} ${path}: ${r.status} ${text.slice(0, 300)}`);
    return (text ? JSON.parse(text) : {}) as T;
  };
  return { name, id: listed.id, proc: p, state, local };
}
const a = await machine("box-a", BOX_A, jake, team);
const b = await machine("box-b", BOX_B, alex);
say(`box-a ${a.id} (jake's, in acme, pid ${a.proc.pid}); box-b ${b.id} (alex's own, pid ${b.proc.pid})`);

// ---- a pane each, and today's thread on box-a's pane (on the daemon)
const run = async (m: typeof a, text: string) => {
  const r = await m.local<{ pane: number }>("POST", "/api/run", { command: `printf '${text}\\n'; exec bash --norc --noprofile`, session: "work" });
  return r.pane;
};
const paneA = await run(a, "running 3 tests\\ntest calc::mean ... FAILED\\n  left: 3\\n right: 2");
const paneB = await run(b, "deploy: staging is at 4f2c1e0, health 200");
await sleep(1500);
const panesA = await a.local<{ id: number; session: number }[]>("GET", "/api/panes");
const sessionA = panesA.find((p) => p.id === paneA)!.session;
await a.local("POST", `/api/threads/pane-${paneA}`, { text: "Before the move: this mean test has been flaky all week.", quote: { pane: paneA, text: "test calc::mean ... FAILED" } });
await a.local("POST", `/api/threads/pane-${paneA}`, { text: "Second note, still on box-a's disk." });
await a.local("POST", `/api/threads/session-${sessionA}`, { text: "A session thread, about the whole work session." });
say(`box-a pane %${paneA} (session ${sessionA}) has an M61 thread of 2 messages and a session thread of 1, on box-a's disk`);

// ---- quoting: the page reads the pane end to end (over the relay), and
// the person picks what to post. Control sees the quote only once posted.
const quoteA = (await jake.capture(a.id, paneA)).split("\n").filter((l) => /FAILED|left|right/.test(l)).join("\n");
const quoteB = (await alex.capture(b.id, paneB)).split("\n").filter((l) => l.startsWith("deploy:")).join("\n");
say("jake captured box-a's pane over the relay:", quoteA);
say("alex captured box-b's pane over the relay:", quoteB);

// ---- both machines stop (SIGTERM by PID, and wait)
for (const m of [a, b]) {
  process.kill(-m.proc.pid!, "SIGTERM");
  await new Promise((r) => (m.proc.exitCode !== null ? r(null) : m.proc.once("exit", r)));
}
await sleep(500);
for (const [who, d] of [["jake", jake], ["alex", alex]] as const) {
  say(`both machines stopped; control's directory, as ${who} sees it:`, (await d.directory()).map((x) => `${x.name} online=${x.online}`));
}

// ---- migration: box-a's old threads, read from its state dir, imported by
// its owner (in the product, the daemon does it signed, on upgrade).
const tdir = join(a.state, "threads");
const threads = readdirSync(tdir)
  .filter((f) => /^(pane|session)-\d+\.jsonl$/.test(f))
  .map((f) => {
    const [, kind, target] = f.match(/^(pane|session)-(\d+)\.jsonl$/)!;
    const messages = readFileSync(join(tdir, f), "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l));
    return { kind, target, messages };
  });
say(`box-a's threads dir: ${readdirSync(tdir).join(", ")}`);
say("one old message as the daemon wrote it:", threads[0]?.messages[0]);
const imported = await jake.api<{ imported: unknown[] }>("/api/chat/import", { machine: a.id, threads });
say("imported:", imported.imported);

// ---- alex's phone turns push on (signed by its device, as the page does)
const phone = createECDH("prime256v1");
phone.generateKeys();
const auth = randomBytes(16);
const endpoint = `http://127.0.0.1:${PUSH}/alex-phone`;
const sub = { v: 1, account: alex.account, device: alex.keys.id, endpoint, p256dh: phone.getPublicKey().toString("base64url"), auth: auth.toString("base64url"), at: Date.now(), sig: "" };
sub.sig = await signText(alex.keys, `illogical push v1\naccount ${sub.account}\ndevice ${sub.device}\nendpoint ${sub.endpoint}\np256dh ${sub.p256dh}\nauth ${sub.auth}\nat ${sub.at}\n`);
await alex.api("/api/push/subscribe", { sub });
say("alex's phone subscribed to push at", endpoint);

// ---- alex's live stream (SSE), held open from here on
const live: { event: string; data: unknown; at: number }[] = [];
const ac = new AbortController();
(async () => {
  const r = await fetch(`${base}/api/chat/live`, { headers: { cookie: alex.cookie }, signal: ac.signal });
  const reader = r.body!.getReader();
  let buf = "";
  for (;;) {
    const { value, done } = await reader.read().catch(() => ({ value: undefined, done: true }));
    if (done) return;
    buf += new TextDecoder().decode(value);
    let i: number;
    while ((i = buf.indexOf("\n\n")) >= 0) {
      const chunk = buf.slice(0, i);
      buf = buf.slice(i + 2);
      const ev = /^event: (.*)$/m.exec(chunk)?.[1];
      const data = /^data: (.*)$/m.exec(chunk)?.[1];
      if (ev) live.push({ event: ev, data: data ? JSON.parse(data) : null, at: Date.now() });
    }
  }
})().catch(() => {});
await sleep(300);

// ---- with both machines off: a channel, a quote, a mention
const { room: general } = await jake.api<{ room: string }>(`/api/chat/teams/${team}/channels`, { name: "general" });
const sent = Date.now();
const p1 = await jake.api<{ seq: number; mentions: string[]; pushed: unknown[] }>(`/api/chat/rooms/${general}/messages`, {
  text: "@alex the mean test fails on box-a, quoted below. Can you look?",
  pane: { machine: a.id, pane: String(paneA) },
  quote: quoteA,
});
say("jake posted in #general:", p1);
for (let i = 0; i < 50 && !live.some((l) => (l.data as { room?: string })?.room === general); i++) await sleep(50);
const got = live.find((l) => (l.data as { room?: string })?.room === general);
say(`alex's live stream got it ${got ? got.at - sent : "never"} ms after the post:`, got);
const p2 = await alex.api(`/api/chat/rooms/${general}/messages`, {
  text: "Looking. Meanwhile staging on my box says this:",
  pane: { machine: b.id, pane: String(paneB) },
  quote: quoteB,
});
say("alex posted in #general, quoting a pane on box-b (his own, not the team's):", p2);

// ---- a DM
const { room: dm } = await alex.api<{ room: string }>(`/api/chat/teams/${team}/dms`, { with: jake.account });
const p3 = await alex.api(`/api/chat/rooms/${dm}/messages`, { text: "Off the channel: is 4f2c1e0 the build you want on staging?" });
say("alex wrote to jake in a DM:", p3);
const p4 = await jake.api(`/api/chat/rooms/${dm}/messages`, { text: "Yes. @alex ship it after the mean fix." });
say("jake answered in the DM:", p4);

// ---- what each sees
const show = (r: { kind: string; name: string; with?: { name: string } | null; unread: number; mentions: number; last: number; read: number }) =>
  `${r.kind} ${r.kind === "dm" ? "with " + r.with?.name : r.name}: last ${r.last}, read ${r.read}, unread ${r.unread}, mentions ${r.mentions}`;
for (const [who, d] of [["jake", jake], ["alex", alex]] as const) {
  const { rooms } = await d.api<{ rooms: Parameters<typeof show>[0][] }>("/api/chat");
  say(`${who}'s rooms:`, rooms.map(show));
}
const asJake = await jake.api<{ messages: { seq: number; name: string; text: string; quote: string | null; pane: unknown; imported: string | null }[] }>(`/api/chat/rooms/${general}/messages`);
say("#general as jake sees it:", asJake.messages.map((m) => ({ seq: m.seq, name: m.name, text: m.text, quote: m.quote, pane: m.pane })));
const asAlex = await alex.api<typeof asJake>(`/api/chat/rooms/${general}/messages`);
say("the pane chips as alex sees them:", asAlex.messages.map((m) => m.pane));

// The imported pane thread: alex (in the team box-a is in) sees it too.
const { rooms: alexRooms } = await alex.api<{ rooms: { id: string; kind: string; machine: string | null; target: string | null }[] }>("/api/chat");
const old = alexRooms.find((r) => r.kind === "pane" && r.machine === a.id && r.target === String(paneA));
if (old) {
  const t = await alex.api<typeof asJake>(`/api/chat/rooms/${old.id}/messages`);
  say(`box-a's pane %${paneA} thread, imported, as alex reads it with box-a off:`, t.messages.map((m) => ({ seq: m.seq, name: m.name, text: m.text, quote: m.quote, imported: m.imported })));
  await alex.api(`/api/chat/rooms/${old.id}/messages`, { text: "Read this with box-a asleep. @jake, the flaky note is from before the move." });
}
// Someone outside the team sees none of it.
const sam = await Device.signIn({ control: base, login: "sam", name: "sam's laptop" });
const samRooms = await sam.api<{ rooms: unknown[] }>("/api/chat");
const samGeneral = await sam.api(`/api/chat/rooms/${general}/messages`).then(() => "read it (wrong)", (e: Error) => e.message);
const samSearch = await sam.api<{ found: unknown[] }>(`/api/chat/search?q=mean`);
say(`sam (not in acme): ${samRooms.rooms.length} rooms, #general: ${samGeneral}, search "mean": ${samSearch.found.length} hits`);
const s1 = await alex.api(`/api/chat/rooms/${general}/read`, { seq: 99 });
say("alex read #general up to the end:", s1);
const found = await jake.api<{ found: { room: string; message: { seq: number; text: string } }[] }>(`/api/chat/search?q=${encodeURIComponent("mean")}`);
say(`jake searched "mean": ${found.found.length} hits:`, found.found.map((f) => `${f.room}#${f.message.seq}: ${f.message.text.slice(0, 60)}`));
const foundQ = await alex.api<typeof found>(`/api/chat/search?q=${encodeURIComponent("FAILED")}`);
say(`alex searched "FAILED" (in quotes too): ${foundQ.found.length} hits`);

// ---- the pushes alex's phone got, opened as the phone would
await sleep(1000);
function openPush(body: Buffer) {
  const salt = body.subarray(0, 16);
  const idlen = body[20];
  const asPublic = body.subarray(21, 21 + idlen);
  const sealed = body.subarray(21 + idlen);
  const info = Buffer.concat([Buffer.from("WebPush: info\0"), phone.getPublicKey(), asPublic]);
  const ikm = Buffer.from(hkdfSync("sha256", phone.computeSecret(asPublic), auth, info, 32));
  const cek = Buffer.from(hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: aes128gcm\0"), 16));
  const nonce = Buffer.from(hkdfSync("sha256", ikm, salt, Buffer.from("Content-Encoding: nonce\0"), 12));
  const d = createDecipheriv("aes-128-gcm", cek, nonce);
  d.setAuthTag(sealed.subarray(-16));
  const plain = Buffer.concat([d.update(sealed.subarray(0, -16)), d.final()]);
  return JSON.parse(plain.subarray(0, plain.lastIndexOf(2)).toString());
}
const pushes = push.pushed.filter((p) => p.path === "/alex-phone").map((p) => openPush(p.body));
say(`alex's phone got ${pushes.length} pushes:`, pushes);
say("live events alex's stream got:", live.map((l) => l.data));
ac.abort();

// ---- what holding pages open costs control: N idle live streams (as alex,
// who sees four rooms), control's resident memory before and after, and
// how long one post takes to reach all of them.
const N = Number(process.env.S41_STREAMS ?? 2000);
const rss = () => Number(/VmRSS:\s+(\d+)/.exec(readFileSync(`/proc/${control.pid}/status`, "utf8"))![1]);
const before = rss();
const streams: AbortController[] = [];
const arrived: number[] = [];
let opened = 0;
for (let i = 0; i < N; i++) {
  (async () => {
    const c = new AbortController();
    streams.push(c);
    const r = await fetch(`${base}/api/chat/live`, { headers: { cookie: alex.cookie }, signal: c.signal });
    opened++;
    const reader = r.body!.getReader();
    for (;;) {
      const { value, done } = await reader.read().catch(() => ({ value: undefined, done: true }));
      if (done) return;
      if (new TextDecoder().decode(value).includes("event: message")) arrived.push(Date.now());
    }
  })().catch(() => {});
}
for (let i = 0; i < 600 && opened < N; i++) await sleep(100);
await sleep(2000);
const during = rss();
const fan = Date.now();
await jake.api(`/api/chat/rooms/${general}/messages`, { text: "One message to every open page." });
for (let i = 0; i < 300 && arrived.length < N; i++) await sleep(50);
say(
  `${opened} idle live streams: control's RSS ${before} kB before, ${during} kB with them open ` +
    `(${(((during - before) * 1024) / Math.max(opened, 1) / 1024).toFixed(1)} kB each); ` +
    `one post reached ${arrived.length} of them, the last ${arrived.length ? Math.max(...arrived) - fan : "-"} ms after it was sent`,
);
for (const c of streams) c.abort();

writeFileSync(join(root, "rig.json"), JSON.stringify({ control: base, team, jake: jake.account, alex: alex.account, machines: { "box-a": a.id, "box-b": b.id }, db: join(root, "control", "control.db") }, null, 2));
say("done; control's database is", join(root, "control", "control.db"));
process.kill(-control.pid!, "SIGTERM");
await new Promise((r) => control.once("exit", r));
stopAll();
process.exit(0);
