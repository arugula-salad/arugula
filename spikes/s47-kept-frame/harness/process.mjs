#!/usr/bin/env node
// S47 (#655): does Chromium run the framed platform page in the keys' process?
// A frame in another process shows up as its own "iframe" target over CDP.
//   node process.mjs [chrome]   (serve.mjs on 8473/8474 is started by the caller)
import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
const here = dirname(fileURLToPath(import.meta.url));
const pw = createRequire(join(here, "../../../web/package.json"))("@playwright/test");
// Playwright turns site isolation off by default, which isn't what people run, so
// this removes that. The last row checks the method: a cross-site frame must read separate.
const channel = process.argv[2] || undefined;
const b = await pw.chromium.launch({
  channel,
  proxy: { server: "http://127.0.0.1:8474" },
  args: ["--ignore-certificate-errors"],
  ignoreDefaultArgs: ["--disable-site-isolation-trials", "--disable-features=IsolateOrigins,site-per-process"],
});
console.log("browser", channel ?? "playwright chromium", b.version());
const browserCdp = await b.newBrowserCDPSession();
async function check(label, shell, frame) {
  const ctx = await b.newContext({ ignoreHTTPSErrors: true });
  const p = await ctx.newPage();
  await p.goto(shell);
  await p.evaluate((src) => new Promise((ok) => { const f = document.createElement("iframe"); f.src = src; f.onload = ok; document.body.append(f); }), frame);
  await p.waitForTimeout(500);
  const { targetInfos } = await browserCdp.send("Target.getTargets");
  const inFrame = await p.frames()[1].evaluate(() => window.originAgentCluster);
  const separate = targetInfos.filter((t) => t.type === "iframe" && t.url.startsWith(new URL(frame).origin)).length > 0;
  console.log(label.padEnd(52), JSON.stringify({ separateProcess: separate, originAgentCluster: inFrame }));
  await ctx.close();
}
const C = "https://control.example.com/", A = "https://app.example.com/?framing=";
await check("today's headers + frame-ancestors control", C, A + "control");
await check("Origin-Agent-Cluster on the frame only", C, A + "control&oac=1");
await check("Origin-Agent-Cluster on both", C + "?oac=1", A + "control&oac=1");
await check("a real cross-site frame, for reference", C, "https://stranger.example.org/?open");
await b.close();
