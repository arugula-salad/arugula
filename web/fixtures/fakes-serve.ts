// The fakes from fakes.ts as one long-running process, for the test
// stack's `control` profile (testnet/compose.yaml): GitHub on 9001, Stripe
// on 9002, a Web Push endpoint on 9003 and Anthropic on 9004 (the inference
// broker's upstream, fake-anthropic.ts), on every interface. With
// `--e2b` (testnet/hosted.yaml), a fake E2B on 9006 (fake-e2b.ts).
//   node --experimental-strip-types fakes-serve.ts [--e2b]

import { fakeAnthropic } from "./fake-anthropic.ts";
import { fakeE2b } from "./fake-e2b.ts";
import { fakeGithub, fakePush, fakeStripe } from "./fakes.ts";

const host = "0.0.0.0";
fakeGithub(9001, host);
fakeStripe(9002, host);
fakePush(9003, host);
fakeAnthropic(9004, host);
if (process.argv.includes("--e2b")) fakeE2b(9006, host);
console.log("fakes: GitHub on 9001, Stripe on 9002, push on 9003, Anthropic on 9004");
for (const s of ["SIGTERM", "SIGINT"] as const) process.on(s, () => process.exit(0));
