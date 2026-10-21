// The fakes from fakes.ts as one long-running process, for the test
// stack's `control` profile (testnet/compose.yaml): GitHub on 9001, Stripe
// on 9002 and a Web Push endpoint on 9003, on every interface. With
// `--managoat` (testnet/window.sh's Cloud), a fake Managoat on 9004 too,
// and on 9005 the SHA-256 of the last secret value put in its vault, which
// control had to decrypt to send: `GET /vault-secret`. Also on 9005, for
// the testnet's handover claim (M144): `POST /hold?name=N` holds the next
// read of the workspace named N (control names each after its Cloud
// workspace) until `POST /release` (or 60 s), and `GET /held` says who sent
// it (the platform machine's address) and lists each request's sender and
// time since `?since=`.
//   node --experimental-strip-types fakes-serve.ts [--managoat]

import { createHash } from "node:crypto";
import { createServer } from "node:http";
import { fakeGithub, fakeManagoat, fakePush, fakeStripe } from "./fakes.ts";

const host = "0.0.0.0";
fakeGithub(9001, host);
fakeStripe(9002, host);
fakePush(9003, host);
const managoat = process.argv.includes("--managoat");
if (managoat) {
  let armed: string | null = null;
  let held: { from: string; at: number; path: string; release: () => void } | null = null;
  const seen: { at: number; from: string; method: string; path: string }[] = [];
  const m = fakeManagoat(9004, host, {
    gate: async (method, path, from) => {
      seen.push({ at: Date.now(), from, method, path });
      if (seen.length > 2000) seen.shift();
      const ws = /^\/api\/workspaces\/([^/]+)$/.exec(path)?.[1];
      if (armed && method === "GET" && ws && m.state().workspaces[ws]?.name === armed) {
        armed = null;
        await new Promise<void>((release) => {
          held = { from, at: Date.now(), path, release };
          setTimeout(release, 60_000);
        });
      }
    },
  });
  createServer((req, res) => {
    const json = (v: unknown) => res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify(v));
    const url = new URL(req.url ?? "/", "http://fakes");
    if (req.method === "POST" && url.pathname === "/hold") {
      armed = url.searchParams.get("name");
      held = null;
      return json({ armed });
    }
    if (req.method === "POST" && url.pathname === "/release") {
      held?.release();
      return json({ released: !!held });
    }
    if (url.pathname === "/held") {
      const since = Number(url.searchParams.get("since") ?? 0);
      return json({ held: held && { from: held.from, at: held.at, path: held.path }, seen: seen.filter((s) => s.at >= since) });
    }
    const secret = m.state().secret;
    json({ sha256: secret ? createHash("sha256").update(secret).digest("hex") : null });
  }).listen(9005, host);
}
console.log(`fakes: GitHub on 9001, Stripe on 9002, push on 9003${managoat ? ", Managoat on 9004" : ""}`);
for (const s of ["SIGTERM", "SIGINT"] as const) process.on(s, () => process.exit(0));
