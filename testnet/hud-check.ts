// hud's own check of a forge assertion (M228's `issuer` claim): hud's
// proxy/forge-signin.ts, read from a hud checkout into DIR by the claim
// (hud is a private repository, so its code isn't kept here), run on an
// assertion the hub issued, as a box configured with SPKI and AUDIENCE runs
// it. One JSON object on stdout, exit 0; or why on stderr, exit 1.
//
//   node --experimental-transform-types testnet/hud-check.ts DIR ASSERTION_FILE SPKI AUDIENCE LOGIN
//
// Checks: hud takes it as `github:LOGIN` (its subject parse); the same
// assertion again is `assertion-replayed`; for another box it's
// `assertion-audience`.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const [dir, file, spki, audience, login] = process.argv.slice(2);
const hud = await import(pathToFileURL(join(dir, "proxy", "forge-signin.ts")).href);
const token = readFileSync(file, "utf8").trim();
const box = (aud: string) => new hud.ForgeSignin(hud.forgeSigninFromEnv({ HUD_FORGE_SIGNIN_KEY: spki, HUD_FORGE_SIGNIN_AUDIENCE: aud }));
const signin = box(audience);
const first = signin.take(token);
const again = signin.take(token);
const other = box(`${audience}-other`).take(token);
const fail = (why: string) => {
  console.error(`hud-check: ${why}: ${JSON.stringify({ first, again, other })}`);
  process.exit(1);
};
if (!first.ok || first.principal !== `github:${login}`) fail("hud didn't take it as the person");
if (again.ok || again.code !== "assertion-replayed") fail("hud took it twice");
if (other.ok || other.code !== "assertion-audience") fail("another box took it");
console.log(JSON.stringify({ principal: first.principal, replayed: again.code, other_box: other.code }));
