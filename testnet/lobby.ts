// A lobby-shaped client of the inference broker (M231) on the testnet: what
// studio's lobby does once it calls the platform (M236), against the fake
// Anthropic in `fakes` (web/fixtures/fake-anthropic.ts). Two people signed in
// with web/fixtures/device-cli.ts: the app's owner (STATE) and the person
// who pays for them (SPONSOR, the lobby's adoption). One JSON object on
// stdout, exit 0; or why on stderr, exit 1.
//
//   node --experimental-strip-types testnet/lobby.ts --state FILE --sponsor FILE --credential C [--break]
//
// As studio, a registered client (its credential C), for a project of the
// owner's team (an app's project):
//   1. A wrong credential is refused (401).
//   2. The owner has no key: the platform's key pays (studio's house key),
//      the answer streams back as Anthropic sent it, and it's metered.
//      $25 of Opus 5 passes studio's $20 a month, so the next call is
//      refused (403 cap) before it goes out, saying when it starts again;
//      the client's view of the owner's month says $25 of $20. (--break:
//      the first call is a small one, so the cap never holds.)
//   3. The owner saves their own key: it pays, with no cap.
//   4. The sponsor's own key (an OAuth token) pays over the owner's when the
//      client names them.
//   5. The project's spend shows each, for its members.
//   6. Anthropic saw the platform's key, then the owner's, then the
//      sponsor's, and nothing of the caller's: no client credential, no
//      cookie, none of the platform's own headers. No answer the client or
//      the owner got holds any key.

import { execFileSync } from "node:child_process";
import { Device } from "../web/fixtures/device.ts";

const args = process.argv.slice(2);
const flag = (name: string) => {
  const at = args.indexOf(name);
  if (at < 0) throw new Error(`${name} is required`);
  return args[at + 1];
};
const broken = args.includes("--break");
const credential = flag("--credential");

/** The platform's key and the people's own: test values, set on the stack
 * (compose.yaml) and here. */
const PLATFORM_KEY = "sk-ant-api03-testnet-platform-key-0123456789"; // gitleaks:allow: a test value
const OWN_KEY = "sk-ant-api03-testnet-owner-key-0123456789"; // gitleaks:allow: a test value
const SPONSOR_KEY = "sk-ant-oat01-testnet-sponsor-token-0123456789"; // gitleaks:allow: a test value
const KEYS = [PLATFORM_KEY, OWN_KEY, SPONSOR_KEY];

const said: string[] = [];

function expect(ok: boolean, what: string, got?: unknown) {
  if (!ok) throw new Error(`${what}${got === undefined ? "" : `: ${typeof got === "string" ? got : JSON.stringify(got)}`}`);
}

/** A call to the broker as studio, for `account`, naming a sponsor when
 * there is one. */
async function lobby(d: Device, path: string, init: { account: string; sponsor?: string; credential?: string; body?: unknown; method?: string }) {
  const headers: Record<string, string> = {
    authorization: `Bearer ${init.credential ?? credential}`,
    "arugula-account": init.account,
    "anthropic-version": "2023-06-01",
    // What a box's agent holds: a stand-in the broker drops.
    "x-api-key": "arugula-brokered",
  };
  if (init.sponsor) headers["arugula-sponsor"] = init.sponsor;
  if (init.body !== undefined) headers["content-type"] = "application/json";
  const res = await fetch(d.reach(d.control + path), {
    method: init.method ?? (init.body === undefined ? "GET" : "POST"),
    headers,
    body: init.body === undefined ? undefined : JSON.stringify(init.body),
  });
  const text = await res.text();
  said.push(JSON.stringify([...res.headers]) + text);
  return { status: res.status, pays: res.headers.get("arugula-pays") ?? "", text, json: () => JSON.parse(text) };
}

/** A call as a signed-in person, through control's site. */
async function person(d: Device, method: string, path: string, body?: unknown) {
  const res = await fetch(d.reach(d.control + path), {
    method,
    headers: { cookie: d.cookie, origin: d.control, ...(body === undefined ? {} : { "content-type": "application/json" }) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  said.push(text);
  expect(res.ok, `${method} ${path}: ${res.status}`, text);
  return JSON.parse(text);
}

const events = (model: string, out: number) =>
  [
    `event: message_start\ndata: {"type":"message_start","message":{"id":"msg_1","model":"${model}","usage":{"input_tokens":100,"output_tokens":1}}}\n\n`,
    `event: content_block_delta\ndata: {"type":"content_block_delta","delta":{"type":"text_delta","text":"hi"}}\n\n`,
    `event: message_delta\ndata: {"type":"message_delta","usage":{"output_tokens":${out}}}\n\n`,
    `event: message_stop\ndata: {"type":"message_stop"}\n\n`,
  ].join("");

const message = (out: number) => ({ model: "claude-opus-5", max_tokens: out, stream: true, messages: [{ role: "user", content: "hi" }] });

/** What the fake Anthropic saw, from inside its container. */
function seen(): { path: string; key: string; headers: string[]; values: string }[] {
  const container = `${process.env.COMPOSE_PROJECT_NAME ?? "arugula-testnet"}-fakes`;
  const js = "fetch('http://127.0.0.1:9004/_testnet/seen').then((r) => r.text()).then((t) => process.stdout.write(t))";
  return JSON.parse(execFileSync("docker", ["exec", container, "node", "-e", js], { encoding: "utf8" }));
}

async function main() {
  const owner = await Device.load(flag("--state"));
  const sponsor = await Device.load(flag("--sponsor"));
  const before = seen().length;
  // The app's project: a team's, the owner its owner (M236: app ownership
  // becomes project membership).
  const team = await owner.createTeam("lobby app");
  let project: string | undefined;
  for (let i = 0; i < 50 && !project; i++) {
    const projects = await person(owner, "GET", "/api/projects");
    project = projects.projects.find((p: { id: string; team: { id: string } | null }) => p.team?.id === team)?.id;
    if (!project) await new Promise((r) => setTimeout(r, 200));
  }
  expect(Boolean(project), "the app's project");
  const base = `/api/projects/${project}/inference/anthropic/v1/messages`;
  const account = owner.account;

  // 1.
  const wrong = await lobby(owner, base, { account, credential: "not-studio", body: message(10) });
  expect(wrong.status === 401 && wrong.json().arugula.reason === "client", "a wrong credential is refused", wrong.text);

  // 2.
  const first = await lobby(owner, base, { account, body: message(broken ? 10 : 1_000_000) });
  expect(first.status === 200 && first.pays === "platform", "the platform's key pays for the owner", first.text);
  expect(first.text === events("claude-opus-5", broken ? 10 : 1_000_000), "streamed through as Anthropic sent it", first.text);
  const capped = await lobby(owner, base, { account, body: message(10) });
  expect(capped.status === 403 && capped.json().arugula?.reason === "cap", "the cap holds: the next call is refused", capped.text);
  const resets: string = capped.json().arugula.resets;
  expect(/^\d{4}-\d{2}-01$/.test(resets), "it says when it starts again", resets);
  const month = (await lobby(owner, "/api/inference/spend", { account })).json();
  expect(month.client === "studio" && month.spent_usd === 25.0005 && month.cap_usd === 20, "the client sees the owner's month", month);

  // 3.
  await person(owner, "PUT", "/api/projects/personal/inference-keys/anthropic", { value: OWN_KEY });
  const own = await lobby(owner, base, { account, body: message(1_000_000) });
  expect(own.status === 200 && own.pays === `account ${account}`, "the owner's own key pays, with no cap", own.text);

  // 4.
  await person(sponsor, "PUT", "/api/projects/personal/inference-keys/anthropic", { value: SPONSOR_KEY });
  const paid = await lobby(owner, base, { account, sponsor: sponsor.account, body: message(10) });
  expect(paid.status === 200 && paid.pays === `account ${sponsor.account}`, "the sponsor's own key pays", paid.text);

  // 5.
  const spend = await person(owner, "GET", `/api/projects/${project}/inference/spend`);
  const line = (pays: string) => spend.lines.find((l: { pays: string; client: string }) => l.pays === pays && l.client === "studio");
  expect(line("platform")?.usd === 25.0005 && line("account")?.calls === 2, "the project's spend shows each", spend);

  // 6.
  const upstream = seen().slice(before);
  const keys = upstream.map((s) => s.key);
  expect(JSON.stringify(keys) === JSON.stringify([PLATFORM_KEY, OWN_KEY, SPONSOR_KEY]), "Anthropic saw each key that paid, in turn", keys.map((k) => k.slice(0, 20)));
  for (const s of upstream) {
    expect(!s.values.includes(credential), "the client's credential never reached Anthropic");
    for (const h of ["cookie", "arugula-account", "arugula-sponsor", "x-arugula-principal"]) expect(!s.headers.includes(h), `Anthropic never got ${h}`, s.headers);
  }
  for (const text of said) for (const k of KEYS) expect(!text.includes(k), "no answer holds a key");
  return { project, account, sponsor: sponsor.account, calls: upstream.length, resets, spend: spend.usd };
}

main().then(
  (v) => {
    console.log(JSON.stringify(v));
    process.exit(0);
  },
  (e: Error) => {
    console.error(`lobby: ${e.message}`);
    process.exit(1);
  },
);
