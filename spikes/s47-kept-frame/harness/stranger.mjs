#!/usr/bin/env node
// S47 (#655): with frame-ancestors naming control alone, another site can't frame
// the platform page, and neither can a sibling on the same site.
//   node stranger.mjs   (serve.mjs on 8473/8474 is started by the caller)
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
const here = dirname(fileURLToPath(import.meta.url));
const pw = createRequire(join(here, "../../../web/package.json"))("@playwright/test");
for (const name of ["chromium", "firefox", "webkit"]) {
  const proxy = { server: "http://127.0.0.1:8474" };
  const b = await pw[name].launch(name === "chromium" ? { proxy, args: ["--ignore-certificate-errors"] } : name === "firefox" ? { proxy, firefoxUserPrefs: { "network.proxy.allow_hijacking_localhost": true } } : { proxy });
  const p = await (await b.newContext({ ignoreHTTPSErrors: true })).newPage();
  for (const shell of ["https://stranger.example.org/?open", "https://control.example.com/"]) {
    await p.goto(shell);
    const r = await p.evaluate(() => new Promise((ok) => {
      const f = document.createElement("iframe");
      f.src = "https://app.example.com/?framing=control";
      addEventListener("message", (e) => { if (e.source === f.contentWindow && e.data?.loaded) ok("framed"); });
      document.body.append(f);
      setTimeout(() => ok("refused"), 3000);
    }));
    console.log(name.padEnd(9), new URL(shell).host.padEnd(24), r);
  }
  await b.close();
}
process.exit(0);
