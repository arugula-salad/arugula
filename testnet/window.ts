// M141's window on the testnet (testnet/window.sh): what a signed-in person
// has in chat and Cloud before the window, and whether it's all still
// there, and still works, after it. The device lives in a state file
// (web/fixtures/device-cli.ts signs it in). One JSON object on stdout,
// exit 0; or why on stderr, exit 1.
//
//   node --experimental-strip-types testnet/window.ts --state FILE COMMAND ...
//
//   seed          chat on, a team with two channels and messages in them
//                 (one word only they hold, for search), a read mark, a
//                 cloud secret, a Cloud workspace that names it, set up, and
//                 a tab with one turn answered: {team, rooms, messages,
//                 word, secret, secret_sha256, workspace, tab}
//   check SEED    everything seed made is listed and reads back the same:
//                 rooms, messages, search, the secret's name, the workspace
//                 and its tab's turn. --has TEXT: a message with this text is
//                 in the first room too; --lacks TEXT: and this one isn't
//   notice TITLE  the page is handed a notice with this title: {notice}
//   write LABEL SEED
//                 new work after the window: a message, another turn in the
//                 tab, and a second workspace that names the secret (the
//                 platform decrypts it to put it in Managoat's vault):
//                 {message, workspace}

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

/** A workspace that names the secret, set up and with its vault filled. */
async function workspace(d: Device, name: string): Promise<string> {
  const w = await must(d, "POST", "/api/cloud/workspaces", {
    request_key: `window-${name}-${Date.now()}`,
    spec: { name, secrets: [SECRET] },
  });
  await until(`workspace ${name} ready`, async () => {
    const r = await must(d, "GET", `/api/cloud/workspaces/${w.id}`);
    if (r.error) throw new Error(`workspace ${name}: ${r.error}`);
    return r.status === "ready" && r.settings_applied ? r : undefined;
  });
  return w.id;
}

/** How many times `answer` is in a tab's events. */
async function answers(d: Device, ws: string, tab: string, answer: string): Promise<number> {
  const r = await must(d, "GET", `/api/cloud/workspaces/${ws}/tabs/${tab}/events`);
  return JSON.stringify(r.events).split(answer).length - 1;
}

/** One turn in a tab: `prompt` goes in (none: the tab's first, sent when
 * it opened), and Managoat's `answer` comes back once more. */
async function turn(d: Device, ws: string, tab: string, prompt: string | null, answer: string): Promise<void> {
  const before = prompt === null ? 0 : await answers(d, ws, tab, answer);
  if (prompt !== null) await must(d, "POST", `/api/cloud/workspaces/${ws}/tabs/${tab}/prompt`, { prompt });
  await until(`the answer "${answer}" in tab ${tab}`, async () => ((await answers(d, ws, tab, answer)) > before ? true : undefined));
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
  const value = `secret-${crypto.randomUUID()}`;
  await must(d, "PUT", `/api/cloud/secrets/${SECRET}`, { value });
  const ws = await workspace(d, "before");
  const agent = (await must(d, "GET", "/api/cloud/agents")).agents[0].id;
  const model = (await must(d, "GET", "/api/cloud/models")).models[0];
  const tab = await must(d, "POST", `/api/cloud/workspaces/${ws}/tabs`, {
    request_key: `window-tab-${Date.now()}`,
    agent_id: agent,
    runtime: model.runtime,
    model: model.model,
    prompt: "Write the docs",
  });
  await turn(d, ws, tab.id, null, "Docs written.");
  const sha = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value));
  return {
    team,
    rooms,
    messages,
    word,
    secret: SECRET,
    secret_sha256: Buffer.from(sha).toString("hex"),
    workspace: ws,
    tab: tab.id,
  };
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
  const secrets = await must(d, "GET", "/api/cloud/secrets");
  if (!JSON.stringify(secrets).includes(s.secret)) throw new Error(`the secret ${s.secret} isn't listed: ${JSON.stringify(secrets)}`);
  const ws = await must(d, "GET", `/api/cloud/workspaces/${s.workspace}`);
  if (ws.status !== "ready") throw new Error(`workspace ${s.workspace}: ${ws.status} ${ws.error ?? ""}`);
  const tabs = (await must(d, "GET", `/api/cloud/workspaces/${s.workspace}/tabs`)).cloud_tabs.map((t: any) => t.id);
  if (!tabs.includes(s.tab)) throw new Error(`tab ${s.tab} isn't listed: ${JSON.stringify(tabs)}`);
  const turns = await answers(d, s.workspace, s.tab, "Docs written.");
  if (turns < 1) throw new Error(`tab ${s.tab} lost its turn`);
  return { rooms: s.rooms.length, messages: total, search: found.length, unread: unread?.unread ?? null, secret: s.secret, workspace: ws.status, turns };
}

async function write(d: Device, label: string, s: any) {
  const text = `after the window: ${label}`;
  await must(d, "POST", `/api/chat/rooms/${s.rooms[0]}/messages`, { text });
  await turn(d, s.workspace, s.tab, "Write more docs", "Docs written.");
  const ws = await workspace(d, `after-${label}`);
  return { message: text, workspace: ws };
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
    default:
      throw new Error(`unknown command ${cmd ?? "(none)"}: seed, check, write`);
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
