#!/usr/bin/env node
// S47 (#655): the prototype in the real shell, against the same build with
// `?noframe`. Needs the demo fleet on same-site hosts:
//   DEV_CONTROL_HOST=arugula.localhost DEV_CONTROL_PLATFORM_HOST=app.arugula.localhost \
//   DEV_CONTROL_PORT=7790 DEV_CONTROL_LOGIN=ruth just demo-fleet --seed 7
//   node measure.mjs behaviour|speed|cost [out dir]

import { createRequire } from "node:module";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const pw = createRequire(join(here, "../../../web/package.json"))("@playwright/test");
const SHELL = "http://arugula.localhost:7790";
const APP = "http://app.arugula.localhost:7790";
const MACHINE = "build-box";
const mode = process.argv[2] || "behaviour";
const out = process.argv[3] || ".";

async function open(browserType = pw.chromium, opts = {}) {
  // launchServer gives the process id cost() needs; only Chromium needs that.
  const chromium = browserType === pw.chromium;
  const server = chromium ? await browserType.launchServer({ args: ["--disable-features=LocalNetworkAccessChecks"] }) : { close: async () => {} };
  const b = chromium ? await browserType.connect(server.wsEndpoint()) : await browserType.launch();
  const ctx = await b.newContext({ viewport: { width: 1440, height: 900 }, ...opts });
  await ctx.addCookies([{ name: "fake_github_login", value: "ruth", domain: "127.0.0.1", path: "/login/oauth" }]);
  const p = await ctx.newPage();
  await p.goto(`${SHELL}/?machine=${MACHINE}`, { waitUntil: "domcontentloaded" });
  await p.getByText("Sign in with GitHub").first().click({ timeout: 60000 });
  await p.getByRole("button", { name: "Sign in as ruth" }).click({ timeout: 3000 }).catch(() => {});
  for (let i = 0; i < 40; i++) {
    await p.waitForTimeout(1000);
    if (await p.locator(".ws-rail").count()) break;
    const m = p.getByRole("button", { name: "It matches" }).first();
    if (await m.isVisible().catch(() => false)) await m.click().catch(() => {});
  }
  await p.waitForSelector(".ws-rail", { timeout: 20000 });
  return { server, b, ctx, p };
}
// A terminal has drawn: any non-blank row in its accessible text or its row layer, else a
// non-blank canvas pixel.
const TERMINAL_DREW = `(() => {
  const x = document.querySelector(".xterm");
  if (!x) return false;
  if ((x.querySelector(".xterm-rows, .xterm-accessibility-tree")?.textContent || "").trim()) return true;
  for (const c of x.querySelectorAll("canvas")) {
    try {
      const g = c.getContext("2d");
      if (!g || !c.width) continue;
      const d = g.getImageData(0, 0, Math.min(c.width, 400), Math.min(c.height, 200)).data;
      for (let i = 3; i < d.length; i += 4) if (d[i] > 0 && (d[i - 1] || d[i - 2] || d[i - 3])) return true;
    } catch {}
  }
  return false;
})()`;
const median = (a) => [...a].sort((x, y) => x - y)[Math.floor(a.length / 2)];
const paints = `new Promise((r)=>requestAnimationFrame(()=>requestAnimationFrame(()=>r(performance.now()))))`;
const log = (k, v) => console.log(k.padEnd(58), typeof v === "string" ? v : JSON.stringify(v));

// A click, then two animation frames: the switch has been painted.
const clickPainted = (p, sel) => p.evaluate(async ([sel, paints]) => {
  const t = performance.now();
  document.querySelector(sel).click();
  await (0, eval)(paints);
  return performance.now() - t;
}, [sel, paints]);

async function panesReady(p) {
  await p.goto(`${SHELL}/?machine=${MACHINE}`);
  await p.waitForSelector(".xterm", { timeout: 30000 });
  await p.waitForTimeout(1500);
}

async function behaviour(type, name) {
  const { server, b, ctx, p } = await open(type);
  const shot = (n) => p.screenshot({ path: join(out, `${name}-${n}.png`) });
  await panesReady(p);
  await p.locator(".xterm").first().evaluate((el) => { el.dataset.kept = "yes"; });
  const sockets = [];
  p.on("websocket", (w) => sockets.push(w.url()));
  const frameErrors = [];
  p.on("console", (m) => { if (m.type() === "error" && /frame|Refused|blocked/i.test(m.text())) frameErrors.push(m.text().slice(0, 160)); });
  await shot("1-panes");

  await p.locator("[data-open-projects]").click();
  const f = p.frameLocator("iframe[data-platform-frame]");
  const t0 = Date.now();
  const list = f.locator(".platform-bar, .app-bar, main").first();
  const signed = await list.waitFor({ timeout: 25000 }).then(() => true).catch(() => false);
  await p.waitForTimeout(2500);
  const fr = p.frames().find((x) => x.url().startsWith(APP));
  log("frame loaded, url", fr?.url() ?? "none");
  log("signed in silently inside the frame", { signed, ms: Date.now() - t0, signInPrompt: signed && (await f.getByText(/sign in/i).count()), bodyText: (await fr?.locator("body").innerText().catch(() => ""))?.slice(0, 120) });
  log("sign-in leftovers in frame url", /code=|#/.test(fr?.url() ?? ""));
  log("frame's own rail hidden", await f.locator(".ws-rail").evaluate((el) => getComputedStyle(el).display).catch(() => "none in DOM"));
  log("shell's rail: Projects lit", await p.locator("[data-open-projects]").evaluate((el) => el.getAttribute("aria-current") ?? el.className));
  await shot("2-projects");

  log("bell over the frame", await p.locator("[data-frame-tools]").count());
  await p.locator("[data-frame-tools] button").first().click().catch((e) => log("bell click failed", String(e).slice(0, 120)));
  await p.waitForTimeout(500);
  await shot("3-bell-open");
  await p.keyboard.press("Escape");
  await p.waitForTimeout(200);

  await f.locator("body").click({ position: { x: 600, y: 500 } });
  await p.keyboard.press("Control+k");
  await p.waitForTimeout(500);
  log("Ctrl+K from inside the frame opens Commands", await p.locator(".palette, [data-palette], [role=dialog]").count());
  await shot("4-commands");
  await p.keyboard.press("Escape");
  await p.waitForTimeout(200);

  await fr.evaluate(() => { history.pushState(null, "", "/boards"); dispatchEvent(new PopStateEvent("popstate")); });
  await p.waitForTimeout(800);
  log("address bar follows the frame", p.url());
  await shot("5-boards");

  await p.locator("[data-open-panes]").click();
  await p.waitForTimeout(400);
  log("back on panes: same terminal element", await p.locator(".xterm").first().evaluate((el) => el.dataset.kept === "yes" && el.getBoundingClientRect().width > 0));
  log("new websockets across the whole trip", sockets.length);
  log("address after Panes", p.url());
  await shot("6-panes-again");

  await p.goBack(); await p.waitForTimeout(500);
  log("Back from panes", { url: p.url(), frameShown: await p.locator("iframe[data-platform-frame]").isVisible() });
  await p.goBack(); await p.waitForTimeout(500);
  log("Back again", { url: p.url(), frameAt: fr.url() });
  await p.goForward(); await p.waitForTimeout(500);
  log("Forward", { url: p.url(), frameAt: fr.url() });

  await p.reload();
  await p.waitForSelector("iframe[data-platform-frame]", { timeout: 20000 }).catch(() => {});
  await p.waitForTimeout(3000);
  log("reload keeps the place", { url: p.url(), frame: p.frames().find((x) => x.url().startsWith(APP))?.url() ?? "none" });
  await shot("7-reloaded");

  // What a compromised platform page can do from the frame.
  const fr2 = p.frames().find((x) => x.url().startsWith(APP));
  if (fr2) {
    log("attack: read parent", await fr2.evaluate(() => { try { return parent.document.title; } catch (e) { return e.name; } }));
    log("attack: fetch control's API", await fr2.evaluate(() => fetch("http://arugula.localhost:7790/control.json", { credentials: "include" }).then((r) => r.status, (e) => e.name)));
    log("attack: say a bad path", await fr2.evaluate(async () => { parent.postMessage({ at: "//evil.test/x" }, "*"); parent.postMessage({ at: "javascript:alert(1)" }, "*"); await new Promise((r) => setTimeout(r, 200)); return "sent"; }));
    log("  address after the bad paths", p.url());
    log("frame's own COOP/XFO", await fr2.evaluate(() => fetch(location.href).then((r) => ({ csp: r.headers.get("content-security-policy").match(/frame-ancestors[^;]*/)[0], xfo: r.headers.get("x-frame-options"), coop: r.headers.get("cross-origin-opener-policy") }))));
    log("frame's sandbox attribute", await p.locator("iframe[data-platform-frame]").getAttribute("sandbox"));
    // Playwright's evaluate counts as a click, the frame has been clicked in too: the worst case.
    log("attack: move the top page, after a click", await fr2.evaluate(() => { try { top.location = "https://example.org/"; return "called"; } catch (e) { return e.name; } }).catch((e) => "detached: " + e.message.slice(0, 60)));
    await p.waitForTimeout(800);
    log("  top page after", p.url());
  }
  log("frame refusals in the console", frameErrors);
  await b.close(); await server.close();
}

async function speed() {
  const { server, b, ctx, p } = await open();
  const today = { toPlatform: [], toPanes: [], firstTerminal: [] };
  // Today, one window (the desktop app, an installed phone app): each way loads a whole page.
  for (let i = 0; i < 6; i++) {
    await p.goto(`${SHELL}/?noframe&machine=${MACHINE}`);
    await p.waitForSelector(".xterm", { timeout: 30000 });
    await p.waitForTimeout(600);
    let t = Date.now();
    await p.goto(`${APP}/`);
    await p.getByText("Yours alone").first().waitFor({ timeout: 30000 });
    today.toPlatform.push(Date.now() - t);
    await p.waitForTimeout(600);
    t = Date.now();
    await p.evaluate((u) => { location.assign(u); }, `${SHELL}/?noframe&machine=${MACHINE}`);
    await p.waitForSelector(".xterm", { timeout: 30000 });
    today.firstTerminal.push(Date.now() - t);
    await p.waitForFunction(TERMINAL_DREW, null, { timeout: 30000 });
    today.toPanes.push(Date.now() - t);
  }
  log("today: panes to platform page, ms", { median: median(today.toPlatform), all: today.toPlatform });
  log("today: platform page to first terminal element, ms", { median: median(today.firstTerminal), all: today.firstTerminal });
  log("today: platform page to terminal showing text, ms", { median: median(today.toPanes), all: today.toPanes });

  for (const hide of ["display", "visibility"]) {
    await p.goto(`${SHELL}/?machine=${MACHINE}&framehide=${hide}`);
    await p.waitForSelector(".xterm", { timeout: 30000 });
    await p.waitForTimeout(1200);
    const t = Date.now();
    await p.locator("[data-open-projects]").click();
    await p.waitForSelector("iframe[data-platform-frame]");
    const f = p.frameLocator("iframe[data-platform-frame]");
    await f.getByText("Yours alone").first().waitFor({ timeout: 30000 });
    log(`frame (${hide}): first open, until the project list, ms`, Date.now() - t);
    await p.waitForTimeout(1000);
    const a = [], c = [];
    for (let i = 0; i < 20; i++) {
      a.push(await clickPainted(p, "[data-open-panes]"));
      await p.waitForTimeout(150);
      c.push(await clickPainted(p, "[data-open-projects]"));
      await p.waitForTimeout(150);
    }
    log(`frame (${hide}): platform to panes, painted, ms`, { median: Math.round(median(a) * 10) / 10, worst: Math.round(Math.max(...a)) });
    log(`frame (${hide}): panes to platform, painted, ms`, { median: Math.round(median(c) * 10) / 10, worst: Math.round(Math.max(...c)) });
  }
  await b.close(); await server.close();
}

// Every process under the browser: memory (PSS) and CPU seconds.
function tree(root) {
  const kids = new Map();
  for (const d of readdirSync("/proc")) {
    if (!/^\d+$/.test(d)) continue;
    try {
      const st = readFileSync(`/proc/${d}/stat`, "utf8");
      const ppid = Number(st.slice(st.lastIndexOf(")") + 2).split(" ")[1]);
      (kids.get(ppid) ?? kids.set(ppid, []).get(ppid)).push(Number(d));
    } catch {}
  }
  const all = [root];
  for (let i = 0; i < all.length; i++) all.push(...(kids.get(all[i]) ?? []));
  return all;
}
function usage(root) {
  let pss = 0, ticks = 0;
  for (const pid of tree(root)) {
    try {
      const m = readFileSync(`/proc/${pid}/smaps_rollup`, "utf8").match(/^Pss:\s+(\d+)/m);
      if (m) pss += Number(m[1]);
      const st = readFileSync(`/proc/${pid}/stat`, "utf8");
      const f = st.slice(st.lastIndexOf(")") + 2).split(" ");
      ticks += Number(f[11]) + Number(f[12]);
    } catch {}
  }
  return { mb: Math.round(pss / 1024), cpuS: ticks / 100 };
}
async function idle(root, seconds) {
  const a = usage(root);
  await new Promise((r) => setTimeout(r, seconds * 1000));
  const z = usage(root);
  return { mb: z.mb, cpuPct: Math.round(((z.cpuS - a.cpuS) / seconds) * 1000) / 10 };
}
async function cost() {
  const SECONDS = 30;
  const { server, b, p } = await open();
  const root = server.process().pid;
  for (const hide of ["display", "visibility"]) {
    await p.goto(`${SHELL}/?machine=${MACHINE}&framehide=${hide}`);
    await p.waitForSelector(".xterm", { timeout: 30000 });
    await p.waitForTimeout(8000);
    log(`${hide}: panes, no frame yet`, await idle(root, SECONDS));
    await p.locator("[data-open-projects]").click();
    await p.waitForTimeout(6000);
    log(`${hide}: frame showing`, await idle(root, SECONDS));
    await p.locator("[data-open-panes]").click();
    await p.waitForTimeout(3000);
    log(`${hide}: frame hidden, panes showing`, await idle(root, SECONDS));
  }
  await b.close(); await server.close();
}

async function heavier() {
  // A first open on a browser that has no platform session yet: the whole hop.
  const { server, b, p } = await open();
  const root = server.process().pid;
  await panesReady(p);
  log("panes, no frame yet", await idle(root, 20));
  const t = Date.now();
  await p.locator("[data-open-projects]").click();
  await p.frameLocator("iframe[data-platform-frame]").getByText("Yours alone").first().waitFor({ timeout: 30000 });
  log("cold first open, hop and all, until the project list, ms", Date.now() - t);
  await p.waitForTimeout(3000);
  log("frame on the project list", await idle(root, 20));
  await p.locator("[data-open-chat]").click();
  await p.waitForTimeout(4000);
  log("frame on chat", await idle(root, 20));
  await p.locator("[data-open-panes]").click();
  await p.waitForTimeout(3000);
  log("chat in the hidden frame, panes showing", await idle(root, 30));
  log("chat's frame still at", p.frames().find((x) => x.url().startsWith(APP))?.url());
  await b.close(); await server.close();
}

if (mode === "heavier") await heavier();
if (mode === "behaviour") await behaviour(pw[process.argv[4] || "chromium"], process.argv[4] || "chromium");
if (mode === "speed") await speed();
if (mode === "cost") await cost();
process.exit(0);
