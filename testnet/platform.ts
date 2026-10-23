// Control's platform on the testnet (M140), as a signed-in person drives it
// through control's own site, whichever process answers. The device lives
// in a state file (web/fixtures/device-cli.ts signs it in). One JSON object
// on stdout, exit 0; or why on stderr, exit 1.
//
//   node --experimental-strip-types testnet/platform.ts --state FILE COMMAND ...
//
//   chat          chat end to end through control: the chat flag on, a team
//                 and a channel, a message posted, read back, and its event
//                 heard on the live stream (SSE); hosted workspaces and billing
//                 answered by the platform: {room, live_ms}
//   hold BOX CONTAINER
//                 one relayed session to BOX's first pane, held open while
//                 CONTAINER restarts: a marker round-trips before and after
//                 on the same socket, which never closes: {held, restart_ms}
//   measure N     each route's latency, N times after a warm-up, one at a
//                 time: {routes: {"GET /api/chat": {p50, p95, p99}, ...}}
//   fanout PORT1 PORT2
//                 two platform machines on one Postgres (M144), each called
//                 directly on its published port with a principal signed with
//                 the stack's key: a stream on each hears a message posted
//                 through the other, and a page that dropped from one resumes
//                 on the other from its Last-Event-ID; exactly one leads:
//                 {room, heard_ms: [1 from 2, 2 from 1], resumed, leader}
//   agents BOX SSH_CONFIG [--break]
//                 agents in conversations (M198), signed by BOX's daemon: the
//                 owner gives BOX threads.read and threads.post over a team's
//                 project (not with --break), @mentions BOX in a channel there;
//                 Claude's view, `arugula mcp` on BOX over ssh, finds the
//                 mention with read_thread, answers with post_thread and sees
//                 capabilities say thread tools are live; the owner reads the
//                 answer labelled as their agent on BOX; once the read grant
//                 is revoked, the next read_thread is refused:
//                 {room, machine, posted}
//   issues-write  native issues before a platform restart (M206): chat on, a
//                 team, two issues in its project (one closed), comments in
//                 the first's thread, read to the first; what was seen:
//                 {project, room, mark, ids, seen}
//   issues-read FILE
//                 after the restart, with issues-write's answer in FILE: the
//                 same issues, timelines, thread and read mark; the next
//                 issue and comment numbered on with no gap; search finds
//                 both: {project, issues, back_ms}

import { execFileSync, spawn } from "node:child_process";
import { readFileSync } from "node:fs";
import { createHmac, randomBytes } from "node:crypto";
import { Device } from "../web/fixtures/device.ts";

const args = process.argv.slice(2);
const at = args.indexOf("--state");
if (at < 0) throw new Error("--state FILE is required");
const state = args.splice(at, 2)[1];
const [cmd, ...rest] = args;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** A call through control as this device's session: its status and JSON. */
async function call(d: Device, method: string, path: string, body?: unknown): Promise<{ status: number; json: any }> {
  const res = await fetch(d.reach(d.control + path), {
    method,
    headers: { cookie: d.cookie, origin: d.control, ...(body === undefined ? {} : { "content-type": "application/json" }) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return { status: res.status, json: await res.json().catch(() => ({})) };
}

async function must(d: Device, method: string, path: string, body?: unknown): Promise<any> {
  const r = await call(d, method, path, body);
  if (r.status < 200 || r.status > 299) throw new Error(`${method} ${path}: ${r.status} ${JSON.stringify(r.json)}`);
  return r.json;
}

/** Chat on, a team of the device's account, and a channel in it. */
async function channel(d: Device): Promise<string> {
  await must(d, "POST", "/api/flags", { name: "chat", on: true });
  const team = await d.createTeam("platform claim");
  const room = await must(d, "POST", `/api/chat/teams/${team}/channels`, { name: `m140-${Date.now() % 100000}` });
  return room.id;
}

async function chat(d: Device) {
  const room = await channel(d);
  // The live stream first: what's posted next arrives on it.
  const live = await fetch(d.reach(d.control + "/api/chat/live"), { headers: { cookie: d.cookie, origin: d.control } });
  if (live.status !== 200) throw new Error(`GET /api/chat/live: ${live.status}`);
  let heard = "";
  const reader = live.body!.getReader();
  (async () => {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) return;
      heard += new TextDecoder().decode(value);
    }
  })().catch(() => {});
  await sleep(300);
  const marker = `M140-${process.pid}-${Date.now()}`;
  const posted = Date.now();
  const sent = await must(d, "POST", `/api/chat/rooms/${room}/messages`, { text: marker });
  const event = `"type":"message","room":"${room}","seq":${sent.seq}`;
  const page = await must(d, "GET", `/api/chat/rooms/${room}/messages`);
  if (!JSON.stringify(page).includes(marker)) throw new Error(`the message isn't in its room: ${JSON.stringify(page)}`);
  while (!heard.includes(event)) {
    if (Date.now() - posted > 10_000) throw new Error(`the live stream didn't carry it: ${heard.slice(-400)}`);
    await sleep(50);
  }
  const liveMs = Date.now() - posted;
  await reader.cancel();
  // Hosted workspaces aren't set up on this stack, so the list is empty:
  // answered by the platform (the hub has no such route of its own).
  const hosted = await call(d, "GET", "/api/workspaces");
  if (hosted.status !== 200 || !Array.isArray(hosted.json.workspaces)) {
    throw new Error(`GET /api/workspaces: ${hosted.status} ${JSON.stringify(hosted.json)}`);
  }
  const billing = await must(d, "GET", "/api/billing");
  if (billing.billing !== true) throw new Error(`GET /api/billing: ${JSON.stringify(billing)}`);
  return { room, live_ms: liveMs };
}

async function hold(d: Device, box: string, container: string) {
  const hit = (await d.directory()).find((x) => x.id === box || x.name === box);
  if (!hit) throw new Error(`control lists no daemon ${box}`);
  const sock = await d.connect(hit.id);
  let closed = "";
  sock.onClose = () => {
    closed = sock.why || "closed";
  };
  const texts: string[] = [];
  let out = "";
  sock.onText = (t) => texts.push(t);
  sock.onBinary = (b) => {
    if (b[0] === 1 || b[0] === 2) out += new TextDecoder().decode(b.subarray(13));
  };
  sock.start();
  const wait = async (what: string, ok: () => boolean, ms = 15_000) => {
    const until = Date.now() + ms;
    while (!ok()) {
      if (closed) throw new Error(`the relayed socket closed (${closed}) waiting for ${what}`);
      if (Date.now() > until) throw new Error(`timed out waiting for ${what}; the pane showed ${JSON.stringify(out.slice(-300))}`);
      await sleep(100);
    }
  };
  await wait("the daemon's hello", () => texts.some((t) => t.includes('"hello"')));
  const hello = JSON.parse(texts.find((t) => t.includes('"hello"'))!);
  const pane: number | undefined = hello.state?.panes?.[0]?.id;
  if (pane === undefined) throw new Error("the daemon has no panes");
  sock.sendText(JSON.stringify({ type: "attach", panes: [{ pane, offset: null }] }));
  const type = (s: string) => {
    const input = new TextEncoder().encode(s);
    const frame = new Uint8Array(13 + input.length);
    frame[0] = 3;
    new DataView(frame.buffer).setUint32(1, pane);
    frame.set(input, 13);
    sock.sendBinary(frame);
  };
  const marker = `HOLD-${process.pid}`;
  type(`echo ${marker}-A-$((6*7))\n`);
  await wait(`${marker}-A-42`, () => out.includes(`${marker}-A-42`));
  const started = Date.now();
  execFileSync("docker", ["restart", container], { stdio: "ignore" });
  const restartMs = Date.now() - started;
  // And a while after it's back, on the same socket.
  await sleep(3000);
  type(`echo ${marker}-B-$((6*7))\n`);
  await wait(`${marker}-B-42`, () => out.includes(`${marker}-B-42`));
  if (closed) throw new Error(`the relayed socket closed: ${closed}`);
  sock.close();
  return { held: true, restart_ms: restartMs };
}

function pct(sorted: number[], p: number): number {
  return Math.round(sorted[Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1)] * 100) / 100;
}

async function measure(d: Device, n: number) {
  const room = await channel(d);
  const posted = await must(d, "POST", `/api/chat/rooms/${room}/messages`, { text: "measure" });
  const routes: [string, string, unknown?][] = [
    ["GET", "/api/me"],
    ["GET", "/api/chat"],
    ["GET", `/api/chat/rooms/${room}/messages`],
    ["GET", "/api/workspaces"],
    ["POST", `/api/chat/rooms/${room}/read`, { seq: posted.seq }],
  ];
  const out: Record<string, { p50: number; p95: number; p99: number }> = {};
  for (const [method, path, body] of routes) {
    for (let i = 0; i < 20; i++) await call(d, method, path, body);
    const times: number[] = [];
    for (let i = 0; i < n; i++) {
      const t = performance.now();
      const r = await call(d, method, path, body);
      times.push(performance.now() - t);
      if (r.status < 200 || r.status > 299) throw new Error(`${method} ${path}: ${r.status} ${JSON.stringify(r.json)}`);
    }
    times.sort((a, b) => a - b);
    const name = `${method} ${path.replace(room, "{id}")}`;
    out[name] = { p50: pct(times, 50), p95: pct(times, 95), p99: pct(times, 99) };
  }
  return { routes: out };
}

/** The stack's key, which control signs principals with
 * (compose.yaml's ARUGULA_PLATFORM_KEY). */
const KEY = Buffer.from(process.env.ARUGULA_TESTNET_PLATFORM_KEY ?? "7e577e577e577e577e577e577e577e577e577e577e577e577e577e577e577e57", "hex");

/** The principal header control's gateway sends with a call (M140): this
 * person, for this method and path, now. */
function vouch(method: string, path: string, account: string): string {
  const json = JSON.stringify({ kind: "person", account });
  const ms = Date.now();
  const id = `m144-${randomBytes(6).toString("hex")}`;
  const tag = createHmac("sha256", KEY).update(`arugula principal v1\n${method}\n${path}\n${ms}\n${id}\n${json}\n`).digest();
  return `v1 ${ms} ${id} ${Buffer.from(json).toString("base64url")} ${tag.toString("base64url")}`;
}

/** A live stream held on the platform at `port`, read in the background. */
async function liveOn(d: Device, port: number, last?: string) {
  const path = "/api/chat/live";
  const headers: Record<string, string> = { "x-arugula-principal": vouch("GET", path, d.account) };
  if (last) headers["last-event-id"] = last;
  const res = await fetch(`http://127.0.0.1:${port}${path}`, { headers });
  if (res.status !== 200) throw new Error(`GET ${path} on :${port}: ${res.status} ${await res.text()}`);
  const stream = { text: "", reader: res.body!.getReader() };
  (async () => {
    for (;;) {
      const { value, done } = await stream.reader.read();
      if (done) return;
      stream.text += new TextDecoder().decode(value);
    }
  })().catch(() => {});
  return stream;
}

async function postOn(d: Device, port: number, room: string, text: string): Promise<number> {
  const path = `/api/chat/rooms/${room}/messages`;
  const res = await fetch(`http://127.0.0.1:${port}${path}`, {
    method: "POST",
    headers: { "x-arugula-principal": vouch("POST", path, d.account), "content-type": "application/json" },
    body: JSON.stringify({ text }),
  });
  if (res.status !== 200) throw new Error(`POST ${path} on :${port}: ${res.status} ${await res.text()}`);
  return (await res.json()).seq;
}

async function hears(stream: { text: string }, what: string, ms = 5_000): Promise<number> {
  const started = Date.now();
  while (!stream.text.includes(what)) {
    if (Date.now() - started > ms) throw new Error(`never heard ${what}: ${stream.text.slice(-400)}`);
    await sleep(10);
  }
  return Date.now() - started;
}

async function fanout(d: Device, ports: number[]) {
  const health = await Promise.all(ports.map(async (p) => (await fetch(`http://127.0.0.1:${p}/platform.json`)).json()));
  if (!health.every((h) => h.database === true)) throw new Error(`a machine can't reach its database: ${JSON.stringify(health)}`);
  const leaders = health.filter((h) => h.leader).length;
  if (leaders !== 1) throw new Error(`${leaders} machines lead: ${JSON.stringify(health)}`);
  const room = await channel(d);
  const event = (seq: number) => `"type":"message","room":"${room}","seq":${seq}`;
  const heard: number[] = [];
  for (const [listen, post] of [ports, [...ports].reverse()]) {
    const s = await liveOn(d, listen);
    await hears(s, '"type":"hello"');
    const seq = await postOn(d, post, room, `M144 through :${post}`);
    heard.push(await hears(s, event(seq)));
    await s.reader.cancel();
  }
  // A page on the first drops; a message goes through the second; the page
  // comes back on the second with the last id it had.
  const s = await liveOn(d, ports[0]);
  await hears(s, '"type":"hello"');
  const seen = await postOn(d, ports[1], room, "seen");
  await hears(s, event(seen));
  const last = s.text.split("\n").filter((l) => l.startsWith("id: ")).pop()!.slice(4);
  await s.reader.cancel();
  const missed = await postOn(d, ports[1], room, "missed while away");
  const back = await liveOn(d, ports[1], last);
  await hears(back, event(missed));
  if (back.text.includes('"type":"resync"')) throw new Error(`resynced instead of resuming from ${last}`);
  await back.reader.cancel();
  return { room, heard_ms: heard, resumed: last, leader: ports[health.findIndex((h) => h.leader)] };
}

/** `arugula mcp` on `box` over ssh: MCP on its stdio, one call at a time. */
function mcpOn(box: string, cfg: string) {
  const p = spawn("ssh", ["-F", cfg, box, 'sh -c "exec $HOME/.local/bin/arugula mcp"'], { stdio: ["pipe", "pipe", "pipe"] });
  let buf = "";
  let err = "";
  const answers = new Map<number, any>();
  p.stdout.on("data", (b) => {
    buf += b.toString();
    for (let i = buf.indexOf("\n"); i >= 0; i = buf.indexOf("\n")) {
      const line = buf.slice(0, i).trim();
      buf = buf.slice(i + 1);
      if (!line) continue;
      const m = JSON.parse(line);
      if (typeof m.id === "number") answers.set(m.id, m);
    }
  });
  p.stderr.on("data", (b) => (err += b.toString()));
  let next = 1;
  const send = (m: unknown) => p.stdin.write(JSON.stringify(m) + "\n");
  const ask = async (method: string, params: unknown, ms = 20_000): Promise<any> => {
    const id = next++;
    send({ jsonrpc: "2.0", id, method, params });
    const until = Date.now() + ms;
    while (!answers.has(id)) {
      if (Date.now() > until || p.exitCode !== null) throw new Error(`mcp on ${box}: no answer to ${method}: ${err.slice(-400)}`);
      await sleep(50);
    }
    const a = answers.get(id);
    if (a.error) throw new Error(`mcp on ${box}: ${method}: ${JSON.stringify(a.error)}`);
    return a.result;
  };
  return {
    async start() {
      await ask("initialize", { protocolVersion: "2025-06-18", capabilities: {}, clientInfo: { name: "testnet", version: "0" } });
      send({ jsonrpc: "2.0", method: "notifications/initialized" });
    },
    /** A tool's answer: its structured content, or why it was refused. */
    async call(name: string, args: unknown): Promise<{ ok: boolean; v: any; text: string }> {
      const r = await ask("tools/call", { name, arguments: args });
      const text = (r.content ?? []).map((c: any) => c.text ?? "").join("\n");
      return { ok: r.isError !== true, v: r.structuredContent ?? {}, text };
    },
    stop() {
      p.stdin.end();
      p.kill();
    },
  };
}

async function agents(d: Device, box: string, cfg: string, broken: boolean) {
  await must(d, "POST", "/api/flags", { name: "chat", on: true });
  const team = await d.createTeam("agents claim");
  const room = (await must(d, "POST", `/api/chat/teams/${team}/channels`, { name: `m198-${Date.now() % 100000}` })).id;
  const projects = await must(d, "GET", "/api/projects");
  const project = projects.projects.find((p: any) => p.team?.id === team)?.id;
  if (!project) throw new Error(`no default project for team ${team}: ${JSON.stringify(projects)}`);
  const machine = (await d.directory()).find((x) => x.name === box)?.id;
  if (!machine) throw new Error(`control lists no daemon ${box}`);
  const grants: Record<string, string> = {};
  if (!broken) {
    for (const power of ["threads.read", "threads.post"]) {
      const g = await must(d, "POST", "/api/grants", { daemon: machine, power, scope: { kind: "project", id: project } });
      grants[power] = g.id;
    }
  }
  const mark = `M198-${process.pid}-${Date.now()}`;
  const asked = await must(d, "POST", `/api/chat/rooms/${room}/messages`, { text: `@${box} look at ${mark}` });
  if (!broken && !asked.mentions.includes(`machine:${machine}`)) throw new Error(`@${box} isn't a mention: ${JSON.stringify(asked)}`);

  const mcp = mcpOn(box, cfg);
  try {
    await mcp.start();
    const read = await mcp.call("read_thread", { room, mentions_only: true });
    if (!read.ok || !read.text.includes(mark)) throw new Error(`read_thread didn't find the mention: ${read.text}`);
    const posted = await mcp.call("post_thread", { room, text: `on it: ${mark}-reply` });
    if (!posted.ok) throw new Error(`post_thread: ${posted.text}`);
    const caps = await mcp.call("capabilities", {});
    if (caps.v?.control?.threads?.live !== true) throw new Error(`capabilities: thread tools aren't live: ${caps.text}`);
    const page = await must(d, "GET", `/api/chat/rooms/${room}/messages`);
    const owner = page.messages.find((m: any) => m.text === `@${box} look at ${mark}`)?.author;
    const reply = page.messages.find((m: any) => m.text === `on it: ${mark}-reply`);
    if (!owner || reply?.author !== owner || reply.agent?.machine !== machine || reply.agent?.machine_name !== box) {
      throw new Error(`the answer isn't labelled as the owner's agent on ${box}: ${JSON.stringify(reply)}`);
    }
    await must(d, "POST", `/api/grants/${grants["threads.read"]}/revoke`);
    const after = await mcp.call("read_thread", { room });
    if (after.ok || !after.text.includes("no threads.read grant")) throw new Error(`read after the revoke: ${after.text}`);
    return { room, machine, posted: posted.v.seq };
  } finally {
    mcp.stop();
  }
}

/** Native issues and the thread on one (M206), written before a platform
 * restart: chat on, a team, two issues in its project (one closed), the
 * first's thread with two comments, read to the first. What `issues-read`
 * checks afterwards. */
async function issuesWrite(d: Device) {
  await must(d, "POST", "/api/flags", { name: "chat", on: true });
  const team = await d.createTeam("issues claim");
  const projects = await must(d, "GET", "/api/projects");
  const project = projects.projects.find((p: any) => p.team?.id === team)?.id;
  if (!project) throw new Error(`no default project for team ${team}: ${JSON.stringify(projects)}`);
  const mark = `m206x${process.pid}x${Date.now()}`;
  const one = await must(d, "POST", `/api/projects/${project}/issues`, { title: `The canary fails ${mark}`, body: "on deploy" });
  const two = await must(d, "POST", `/api/projects/${project}/issues`, { title: "Flaky login test" });
  await must(d, "PATCH", `/api/issues/${two.id}`, { state: "closed" });
  const room = (await must(d, "POST", "/api/chat/targets", { kind: "object", target: one.id })).id;
  for (const text of [`first look ${mark}`, "second look"]) await must(d, "POST", `/api/chat/rooms/${room}/messages`, { text });
  await must(d, "POST", `/api/chat/rooms/${room}/read`, { seq: 1 });
  const seen = await issuesSeen(d, project, room, [one.id, two.id]);
  return { project, room, mark, ids: [one.id, two.id], seen };
}

/** What a reader sees of the project's issues, their timelines, the thread
 * and its unread count. */
async function issuesSeen(d: Device, project: string, room: string, ids: string[]) {
  const timelines = [];
  for (const id of ids) timelines.push(await must(d, "GET", `/api/issues/${id}/events`));
  const rooms = await must(d, "GET", "/api/chat");
  const mine = rooms.rooms.find((r: any) => r.id === room);
  return {
    issues: await must(d, "GET", `/api/projects/${project}/issues?state=all`),
    timelines,
    thread: await must(d, "GET", `/api/chat/rooms/${room}/messages`),
    unread: mine ? [mine.unread, mine.mentions] : null,
  };
}

/** After the restart (`issues-write`'s answer in FILE): the same issues,
 * timelines, thread and read mark; the next issue and comment numbered on
 * with no gap; search finds the first issue and its comment. */
async function issuesRead(d: Device, file: string) {
  const was = JSON.parse(readFileSync(file, "utf8"));
  // Until the platform answers again, control's gateway says 502 or 503.
  const started = Date.now();
  for (;;) {
    const r = await call(d, "GET", `/api/projects/${was.project}/issues?state=all`);
    if (r.status !== 502 && r.status !== 503) break;
    if (Date.now() - started > 30_000) throw new Error(`the platform didn't come back: ${r.status}`);
    await sleep(500);
  }
  const back_ms = Date.now() - started;
  const seen = await issuesSeen(d, was.project, was.room, was.ids);
  if (JSON.stringify(seen) !== JSON.stringify(was.seen)) {
    throw new Error(`what reads back isn't what was written:\n${JSON.stringify(seen)}\n${JSON.stringify(was.seen)}`);
  }
  const three = await must(d, "POST", `/api/projects/${was.project}/issues`, { title: "After the restart" });
  if (three.number !== 3) throw new Error(`the next issue is #${three.number}, not #3`);
  const sent = await must(d, "POST", `/api/chat/rooms/${was.room}/messages`, { text: "third look" });
  if (sent.seq !== 3) throw new Error(`the next comment is ${sent.seq}, not 3`);
  const found = await must(d, "GET", `/api/projects/${was.project}/issues?q=${was.mark}`);
  if (found.issues?.[0]?.id !== was.ids[0]) throw new Error(`search didn't find the issue: ${JSON.stringify(found)}`);
  const hits = await must(d, "GET", `/api/chat/search?q=${was.mark}`);
  if (!hits.found?.some((h: any) => h.room === was.room)) throw new Error(`search didn't find the comment: ${JSON.stringify(hits)}`);
  return { project: was.project, issues: 3, back_ms };
}

async function main() {
  const d = await Device.load(state);
  switch (cmd) {
    case "chat":
      return chat(d);
    case "hold":
      return hold(d, rest[0], rest[1]);
    case "measure":
      return measure(d, Number(rest[0] ?? 300));
    case "fanout":
      return fanout(d, [Number(rest[0]), Number(rest[1])]);
    case "agents":
      return agents(d, rest[0], rest[1], rest.includes("--break"));
    case "issues-write":
      return issuesWrite(d);
    case "issues-read":
      return issuesRead(d, rest[0]);
    default:
      throw new Error(`unknown command ${cmd ?? "(none)"}: chat, hold, measure, fanout, agents, issues-write, issues-read`);
  }
}

main().then(
  (v) => {
    console.log(JSON.stringify(v));
    process.exit(0);
  },
  (e: Error) => {
    console.error(`platform: ${e.message}`);
    process.exit(1);
  },
);
