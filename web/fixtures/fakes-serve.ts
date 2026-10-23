// The fakes from fakes.ts as one long-running process, for the test
// stack's `control` profile (testnet/compose.yaml): GitHub on 9001, Stripe
// on 9002 and a Web Push endpoint on 9003, on every interface. With
// `--e2b` (testnet/hosted.yaml), a fake E2B on 9006 (fake-e2b.ts). With
// Infisical's captured answers mounted at /infisical (testnet/compose.yaml),
// a fake Infisical on 9007 for the platform's projection (M235).
//   node --experimental-strip-types fakes-serve.ts [--e2b]

import { existsSync } from "node:fs";
import { fakeE2b } from "./fake-e2b.ts";
import { fakeInfisical } from "./fake-infisical.ts";
import { fakeGithub, fakePush, fakeStripe } from "./fakes.ts";

const host = "0.0.0.0";
fakeGithub(9001, host);
fakeStripe(9002, host);
fakePush(9003, host);
if (process.argv.includes("--e2b")) fakeE2b(9006, host);
if (existsSync("/infisical/answers.json")) fakeInfisical(9007, host, "/infisical/answers.json", "infisical-testnet-secret");
console.log("fakes: GitHub on 9001, Stripe on 9002, push on 9003");
for (const s of ["SIGTERM", "SIGINT"] as const) process.on(s, () => process.exit(0));
