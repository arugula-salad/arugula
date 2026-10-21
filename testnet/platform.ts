// Control's platform on the testnet (M140), as a signed-in person drives it
// through control's own site, whichever process answers. The device lives
// in a state file (web/fixtures/device-cli.ts signs it in). One JSON object
// on stdout, exit 0; or why on stderr, exit 1.
//
//   node --experimental-strip-types testnet/platform.ts --state FILE COMMAND ...
//
//   chat          chat end to end through control: the chat flag on, a team
//                 and a channel, a message posted, read back, and its event
//                 heard on the live stream (SSE); Cloud and billing answered by the
//                 platform: {room, live_ms}
//   hold BOX CONTAINER
//                 one relayed session to BOX's first pane, held open while
//                 CONTAINER restarts: a marker round-trips before and after
//                 on the same socket, which never closes: {held, restart_ms}
//   measure N     each route's latency, N times after a warm-up, one at a
//                 time: {routes: {"GET /api/chat": {p50, p95, p99}, ...}}

import { execFileSync } from "node:child_process";
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
  // Cloud has no Managoat on the testnet, so the list is empty: answered by
  // the platform (the hub has no Cloud route of its own to answer with).
  const cloud = await call(d, "GET", "/api/cloud/workspaces");
  if (cloud.status !== 200 || !Array.isArray(cloud.json.cloud_workspaces)) {
    throw new Error(`GET /api/cloud/workspaces: ${cloud.status} ${JSON.stringify(cloud.json)}`);
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
    ["GET", "/api/cloud/workspaces"],
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

async function main() {
  const d = await Device.load(state);
  switch (cmd) {
    case "chat":
      return chat(d);
    case "hold":
      return hold(d, rest[0], rest[1]);
    case "measure":
      return measure(d, Number(rest[0] ?? 300));
    default:
      throw new Error(`unknown command ${cmd ?? "(none)"}: chat, hold, measure`);
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
