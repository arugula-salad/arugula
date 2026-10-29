// Real usage through a relay, for billing's test-mode check (#318's
// P11; docs in ../README.md, "Billing"): N people join a room of an app by
// room token, then two of them hold a voice call for S seconds (the relay
// needs LIVEKIT_* set for calls; any values do, nothing connects to them).
// The relay meters it as P9 does; its billing reporter then tells Stripe.
//
//   APP_SECRET=… node scripts/billing_usage.mjs --relay http://127.0.0.1:4212 \
//     --app app_0123456789abcdef --people 520 --call-seconds 125
//
// The app's server secret comes from the environment (APP_SECRET), never
// argv. Node 22+ (a global WebSocket); the phoenix client from deps/.

import { createHmac } from "node:crypto";
import { parseArgs } from "node:util";
import { Socket } from "../deps/phoenix/priv/static/phoenix.mjs";

const { values: o } = parseArgs({
  options: {
    relay: { type: "string", default: "http://127.0.0.1:4212" },
    app: { type: "string" },
    people: { type: "string", default: "10" },
    "call-seconds": { type: "string", default: "0" },
    prefix: { type: "string", default: `bill-${Date.now().toString(36)}` },
  },
});
const secret = process.env.APP_SECRET;
if (!o.app || !secret) {
  console.error("Needs --app and APP_SECRET (the app's server secret).");
  process.exit(2);
}

const b64 = (x) => Buffer.from(typeof x === "string" ? x : JSON.stringify(x)).toString("base64url");
const token = (room, sub) => {
  const data = `${b64({ alg: "HS256", typ: "JWT" })}.${b64({ app: o.app, room, sub, name: sub, exp: Math.floor(Date.now() / 1000) + 3600 })}`;
  return `${data}.${createHmac("sha256", secret).update(data).digest("base64url")}`;
};
const ws = o.relay.replace(/^http/, "ws") + "/socket";
const room = `${o.prefix}-room`;

/** One person in the room: `{socket, channel}` once joined. */
function join(sub) {
  return new Promise((resolve, reject) => {
    const socket = new Socket(ws, { params: { token: token(room, sub) } });
    socket.connect();
    const channel = socket.channel(`room:${room}`, {});
    channel
      .join()
      .receive("ok", () => resolve({ socket, channel }))
      .receive("error", (e) => (socket.disconnect(), reject(new Error(`${sub}: ${JSON.stringify(e)}`))));
  });
}

const people = Number(o.people);
let joined = 0;
let refused = 0;
// In batches, each person leaving once they're counted.
for (let i = 0; i < people; i += 25) {
  const batch = [];
  for (let k = i; k < Math.min(i + 25, people); k++) batch.push(join(`${o.prefix}-p${k}`));
  for (const r of await Promise.allSettled(batch)) {
    if (r.status === "fulfilled") {
      joined++;
      r.value.socket.disconnect();
    } else {
      refused++;
      if (refused <= 3) console.error(r.reason.message);
    }
  }
}
console.log(`joined ${joined}, refused ${refused}`);

const secs = Number(o["call-seconds"]);
if (secs > 0) {
  const a = await join(`${o.prefix}-caller`);
  const b = await join(`${o.prefix}-callee`);
  const id = await new Promise((resolve, reject) => {
    b.channel.on("call", ({ id }) => b.channel.push("call_answer", { id, yes: true }));
    a.channel.on("call_join", ({ id }) => resolve(id));
    a.channel.push("call", { to: `${o.prefix}-callee` }).receive("error", (e) => reject(new Error(`call: ${JSON.stringify(e)}`)));
    setTimeout(() => reject(new Error("the call didn't start (LIVEKIT_* unset on the relay?)")), 10_000);
  });
  console.log(`in a call for ${secs} s…`);
  await new Promise((r) => setTimeout(r, secs * 1000));
  a.channel.push("call_leave", { id });
  b.channel.push("call_leave", { id });
  await new Promise((r) => setTimeout(r, 500));
  a.socket.disconnect();
  b.socket.disconnect();
  console.log("call over");
}
process.exit(0);
