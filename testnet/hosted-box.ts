// A hosted box for testnet's `hosted` claim (M178), until `arugulad
// --hosted` (M180) does this itself: it makes a machine's keys, enrolls
// once with a ticket or a sandbox id at control's /api/hosted/enroll, and,
// enrolled, dials the relay signed as the machine and holds the socket so
// people can be routed to it. Not a daemon: it serves nothing over the
// socket.
//
//   node --experimental-strip-types hosted-box.ts --control URL [--via FROM=TO]...
//        (--ticket T | --sandbox S) [--hold SECS]
//
// Prints one JSON line: {status, device?, error?} for the enrollment, then
// {dialed: true} once it's on the relay; exits after --hold (0: at once).

import { generateKeys, signText } from "../web/src/shell/e2e/keys.ts";
import { hex } from "../web/src/shell/e2e/cert.ts";

const args = process.argv.slice(2);
const opt = (name: string): string[] => {
  const out: string[] = [];
  for (let i = args.indexOf(name); i >= 0; i = args.indexOf(name)) {
    out.push(args[i + 1]);
    args.splice(i, 2);
  }
  return out;
};
const [control] = opt("--control");
const via = Object.fromEntries(opt("--via").map((v) => [v.slice(0, v.indexOf("=")), v.slice(v.indexOf("=") + 1)]));
const [ticket] = opt("--ticket");
const [sandbox] = opt("--sandbox");
const hold = Number(opt("--hold")[0] ?? "0");
const reach = (u: string) => {
  for (const [from, to] of Object.entries(via)) if (u.startsWith(from)) return to + u.slice(from.length);
  return u;
};
const sha256 = async (s: string) => hex(new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(s))));
const out = (v: unknown) => console.log(JSON.stringify(v));

async function main() {
  if (!control || !(ticket || sandbox)) throw new Error("--control URL and --ticket T or --sandbox S");
  const keys = await generateKeys(true);
  const at = Date.now();
  const credential = await sha256(ticket ?? sandbox);
  const proof = await signText(keys, `arugula hosted enroll v1\nenrollment ${credential}\nnoise ${keys.noisePub}\nsign ${keys.signPub}\nat ${at}\n`);
  const res = await fetch(reach(`${control}/api/hosted/enroll`), {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ ticket, sandbox, noise: keys.noisePub, sign: keys.signPub, at, proof }),
  });
  const body = (await res.json().catch(() => ({}))) as { error?: string; cert?: { device: string } };
  out({ status: res.status, device: body.cert?.device, error: body.error });
  if (!res.ok || hold <= 0) return;
  // Dial the relay signed as the machine (`illogical daemon auth v2`).
  const path = "/api/relay/dial";
  const ms = Date.now();
  const nonce = hex(crypto.getRandomValues(new Uint8Array(16)));
  const sig = await signText(keys, `illogical daemon auth v2\nGET\n${path}\n${ms}\n${nonce}\n${await sha256("")}\n`);
  const Ws = globalThis.WebSocket as unknown as new (u: string, o: unknown) => WebSocket;
  const ws = new Ws(reach(control).replace(/^http/, "ws") + path, { headers: { "x-arugula-auth": `v2 ${keys.id} ${ms} ${nonce} ${sig}` } });
  await new Promise<void>((res, rej) => {
    ws.onopen = () => res();
    ws.onerror = () => rej(new Error("the relay refused the machine's socket"));
  });
  out({ dialed: true });
  await new Promise((r) => setTimeout(r, hold * 1000));
  ws.close();
}

main().then(
  () => process.exit(0),
  (e: Error) => {
    console.error(`hosted-box: ${e.message}`);
    process.exit(1);
  },
);
