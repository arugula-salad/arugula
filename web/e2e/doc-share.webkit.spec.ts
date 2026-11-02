// #506 in WebKit (docs/testing.md "Phones"): a shared Doc's ```prototype section
// runs in a frame on control's proto origin, under a page whose
// `script-src 'self'` would stop a srcdoc frame's scripts, and gets out of
// nowhere. The same checks as doc-share.spec.ts's, on an iPhone: a real
// control with a block domain, and a visitor with nothing carried over.

import { test } from "@playwright/test";
import { DOMAIN, prototypeInViewer, rig, type Rig } from "./doc-share-rig";
import { attacker } from "./doc-steps";
import { iphone } from "./phones";
import "./local-token";

test.describe.configure({ mode: "serial" });

let r: Rig;
test.beforeAll(async () => {
  test.setTimeout(120_000);
  r = await rig(DOMAIN);
});
test.afterAll(async () => {
  await r?.dispose();
});

for (const scheme of ["light", "dark"] as const) {
  test(`a shared Doc's prototype runs in WebKit on an iPhone and gets out of nowhere, ${scheme}`, async ({ browser }) => {
    const a = await attacker();
    try {
      await prototypeInViewer(r, browser, a, scheme, iphone);
    } finally {
      a.close();
    }
  });
}
