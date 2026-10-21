// The fakes from fakes.ts as one long-running process, for the test
// stack's `control` profile (testnet/compose.yaml): GitHub on 9001, Stripe
// on 9002 and a Web Push endpoint on 9003, on every interface. With
// `--managoat` (testnet/window.sh's Cloud), a fake Managoat on 9004 too,
// and on 9005 the SHA-256 of the last secret value put in its vault, which
// control had to decrypt to send: `GET /vault-secret`.
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
  const m = fakeManagoat(9004, host);
  createServer((_, res) => {
    const secret = m.state().secret;
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ sha256: secret ? createHash("sha256").update(secret).digest("hex") : null }));
  }).listen(9005, host);
}
console.log(`fakes: GitHub on 9001, Stripe on 9002, push on 9003${managoat ? ", Managoat on 9004" : ""}`);
for (const s of ["SIGTERM", "SIGINT"] as const) process.on(s, () => process.exit(0));
