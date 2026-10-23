// M141's window on the testnet (testnet/window.sh): what a signed-in person
// has in chat and a project's secrets before the window, and whether it's all still
// there, and still works, after it. The device lives in a state file
// (web/fixtures/device-cli.ts signs it in). One JSON object on stdout,
// exit 0; or why on stderr, exit 1.
//
//   node --experimental-strip-types testnet/window.ts --state FILE COMMAND ...
//
//   seed          chat on, a team with two channels and messages in them
//                 (one word only they hold, for search), a read mark, and an
//                 environment in the team's project with a secret set (sealed
//                 with the project's key, which the master key wraps): {team,
//                 rooms, messages, word, project, environment, secret}
//   check SEED    everything seed made is listed and reads back the same:
//                 rooms, messages, search, the environment and its secret's
//                 name. --has TEXT: a message with this text is in the first
//                 room too; --lacks TEXT: and this one isn't
//   notice TITLE  the page is handed a notice with this title: {notice}
//   write LABEL SEED
//                 new work after the window: a message, and the secret set
//                 again (the platform seals it with the project's key, which
//                 it opens with the master key): {message}
//   gateway N SEED
//                 through control's gateway, as a page has it (M145, two
//                 machines behind it): a live stream, then N messages posted
//                 in the first room, each heard on the stream:
//                 {room, heard, heard_ms}

import { readFileSync } from "node:fs";
import { Device } from "../web/fixtures/device.ts";

const args = process.argv.slice(2);
const at = args.indexOf("--state");
if (at < 0) throw new Error("--state FILE is required");
const state = args.splice(at, 2)[1];
const flag = (name: string) => {
  const i = args.indexOf(name);
  return i < 0 ? undefined : args.splice(i, 2)[1];
};
const has = flag("--has");
const lacks = flag("--lacks");
const [cmd, ...rest] = args;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const SECRET = "WINDOW_TOKEN";

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

async function until<T>(what: string, f: () => Promise<T | undefined>, ms = 60_000): Promise<T> {
  const end = Date.now() + ms;
  let last: unknown;
  for (;;) {
    try {
      const v = await f();
      if (v !== undefined) return v;
    } catch (e) {
      last = e;
    }
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}${last ? `: ${(last as Error).message}` : ""}`);
    await sleep(500);
  }
}

/** Set the environment's secret to a new value, as an owner in a browser. */
async function setSecret(d: Device, environment: string): Promise<void> {
  await must(d, "POST", `/api/environments/${environment}/secrets/${SECRET}`, { value: `secret-${crypto.randomUUID()}` });
}

async function seed(d: Device) {
  await must(d, "POST", "/api/flags", { name: "chat", on: true });
  const team = await d.createTeam("window rehearsal");
  const word = `windowword${Date.now() % 100000}`;
  const rooms: string[] = [];
  const messages: Record<string, string[]> = {};
  for (const name of ["general-w", "ops-w"]) {
    const room = (await must(d, "POST", `/api/chat/teams/${team}/channels`, { name })).id;
    rooms.push(room);
    messages[room] = [];
    for (let i = 1; i <= 3; i++) {
      const text = i === 2 ? `before the window, ${word} ${name}` : `${name} message ${i} before the window`;
      await must(d, "POST", `/api/chat/rooms/${room}/messages`, { text });
      messages[room].push(text);
    }
  }
  const last = (await must(d, "GET", `/api/chat/rooms/${rooms[0]}/messages`)).messages.at(-1).seq;
  await must(d, "POST", `/api/chat/rooms/${rooms[0]}/read`, { seq: last });
  const project = (await must(d, "GET", "/api/projects")).projects.find((p: any) => p.team?.id === team)?.id;
  if (!project) throw new Error(`no default project for team ${team}`);
  const env = await must(d, "POST", `/api/projects/${project}/environments`, {
    name: "window",
    driver: "window-rehearsal",
    secret_names: [SECRET],
  });
  await setSecret(d, env.id);
  return { team, rooms, messages, word, project, environment: env.id, secret: SECRET };
}

async function check(d: Device, s: any) {
  const listed = (await must(d, "GET", "/api/chat")).rooms.map((r: any) => r.id);
  for (const r of s.rooms) if (!listed.includes(r)) throw new Error(`room ${r} isn't listed: ${JSON.stringify(listed)}`);
  let total = 0;
  for (const r of s.rooms) {
    const page = await must(d, "GET", `/api/chat/rooms/${r}/messages`);
    const texts: string[] = page.messages.map((m: any) => m.text);
    for (const t of s.messages[r]) if (!texts.includes(t)) throw new Error(`room ${r} lost "${t}": ${JSON.stringify(texts)}`);
    if (r === s.rooms[0] && has && !texts.includes(has)) throw new Error(`room ${r} hasn't "${has}"`);
    if (r === s.rooms[0] && lacks && texts.includes(lacks)) throw new Error(`room ${r} has "${lacks}"`);
    total += texts.length;
  }
  const found = (await must(d, "GET", `/api/chat/search?q=${s.word}`)).found;
  if (found.length !== 2) throw new Error(`search for ${s.word} found ${found.length}, not 2: ${JSON.stringify(found)}`);
  const unread = (await must(d, "GET", "/api/chat")).rooms.find((r: any) => r.id === s.rooms[0]);
  const envs = JSON.stringify(await must(d, "GET", `/api/projects/${s.project}/environments`));
  if (!envs.includes(s.environment) || !envs.includes(s.secret)) {
    throw new Error(`the environment ${s.environment} or its secret ${s.secret} isn't listed: ${envs}`);
  }
  return { rooms: s.rooms.length, messages: total, search: found.length, unread: unread?.unread ?? null, environment: s.environment, secret: s.secret };
}

async function write(d: Device, label: string, s: any) {
  const text = `after the window: ${label}`;
  await must(d, "POST", `/api/chat/rooms/${s.rooms[0]}/messages`, { text });
  await setSecret(d, s.environment);
  return { message: text };
}

async function gateway(d: Device, n: number, s: any) {
  const room = s.rooms[0];
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
  await until("the stream's hello", async () => (heard.includes('"type":"hello"') ? true : undefined), 10_000);
  const ms: number[] = [];
  for (let i = 1; i <= n; i++) {
    const posted = Date.now();
    const sent = await must(d, "POST", `/api/chat/rooms/${room}/messages`, { text: `through the gateway ${i} of ${n}` });
    const event = `"type":"message","room":"${room}","seq":${sent.seq}`;
    while (!heard.includes(event)) {
      if (Date.now() - posted > 10_000) throw new Error(`the live stream didn't carry seq ${sent.seq}: ${heard.slice(-400)}`);
      await sleep(10);
    }
    ms.push(Date.now() - posted);
  }
  await reader.cancel();
  return { room, heard: ms.length, heard_ms: ms };
}

async function main() {
  const d = await Device.load(state);
  const seeded = () => JSON.parse(readFileSync(rest.at(-1)!, "utf8"));
  switch (cmd) {
    case "seed":
      return seed(d);
    case "check":
      return check(d, seeded());
    case "notice": {
      const r = await must(d, "GET", "/api/teams");
      const n = (r.notices ?? []).find((x: any) => x.title === rest[0]);
      if (!n) throw new Error(`no notice "${rest[0]}": ${JSON.stringify(r.notices)}`);
      return { notice: n.title, body: n.body };
    }
    case "write":
      return write(d, rest[0], seeded());
    case "gateway":
      return gateway(d, Number(rest[0]), seeded());
    default:
      throw new Error(`unknown command ${cmd ?? "(none)"}: seed, check, notice, write, gateway`);
  }
}

main().then(
  (v) => {
    console.log(JSON.stringify(v));
    process.exit(0);
  },
  (e: Error) => {
    console.error(`window: ${e.message}`);
    process.exit(1);
  },
);
