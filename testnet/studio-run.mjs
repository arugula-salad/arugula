// studio's lobby on the testnet (M236's `studio` claim), run in a container
// on the stack's networks so it reaches the hub as its issuer names itself
// (control's inner address). studio is a private repository: its code is
// read from the checkout mounted at /studio (ARUGULA_TESTNET_STUDIO), never
// kept here. This starts studio's own fake loam (lobby/fake-loam.mjs) with
// a box template, then the lobby itself (lobby/server.mjs) with the
// environment the claim gives it, and the loam it just started.
//
//   node /run/studio-run.mjs

import { mkdirSync, writeFileSync } from "node:fs";

const studio = process.env.STUDIO_CHECKOUT ?? "/studio";
const { fakeLoam } = await import(`${studio}/lobby/fake-loam.mjs`);
const token = "loam_testnettestnettestnettestnet";
const loam = await fakeLoam({ token });
loam.add("studio-box-template", { labels: { lobby: "studio", kind: "template" } });
loam.checkpoints.set("studio-box-template", [{ id: "v1", comment: "box template (testnet)", memory: false, auto: false }]);

const state = process.env.STUDIO_STATE ?? "/tmp/studio/state";
mkdirSync(state, { recursive: true });
writeFileSync("/tmp/studio/studio-secret", "testnet-shared-secret");
writeFileSync("/tmp/studio/lobby-secret", "testnet-lobby-secret");
Object.assign(process.env, {
  LOAM_API_URL: loam.url,
  LOAM_KEY: token,
  STUDIO_STATE: state,
  STUDIO_SECRET_FILE: "/tmp/studio/studio-secret",
  LOBBY_SECRET_FILE: "/tmp/studio/lobby-secret",
});
await import(`${studio}/lobby/server.mjs`);
