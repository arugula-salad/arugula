// Measurement only (not in CI): divider drag and tab switch with many busy
// panes. Prints one JSON line per run: `PERF {...}`.
// Env: PERF_PANES (default 8), PERF_BUSY (default 5), PERF_RUNS, PERF_LABEL.
import { appendFileSync, writeFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { open, reset, ready, panes } from "./helpers";

const PANES = Number(process.env.PERF_PANES ?? 8);
const BUSY = Number(process.env.PERF_BUSY ?? 5);
const SWITCHES = Number(process.env.PERF_SWITCHES ?? 10);
const LABEL = process.env.PERF_LABEL ?? "run";
const SENTINEL = "/tmp/perf-sentinel-" + process.pid;

test.use({ viewport: { width: 1400, height: 900 } });
test.setTimeout(240_000);

const RTT = Number(process.env.PERF_RTT ?? 0);

async function instrument(page: Page) {
  if (RTT > 0) {
    // Each direction waits RTT/2; order is kept (equal delays).
    await page.routeWebSocket(/\/ws(\?|$)/, (ws) => {
      const server = ws.connectToServer();
      ws.onMessage((m) => setTimeout(() => server.send(m), RTT / 2));
      server.onMessage((m) => setTimeout(() => ws.send(m), RTT / 2));
      ws.onClose((c, r) => server.close({ code: c, reason: r }));
      server.onClose((c, r) => ws.close({ code: c, reason: r }));
    });
  }
  await page.addInitScript(() => {
    const w = window as any;
    w.__perf = { msgs: 0, bytes: 0, sizes: [] as number[], resize: 0, sent: 0, long: [] as number[], mut: 0, frames: [] as number[] };
    const Orig = window.WebSocket;
    w.WebSocket = class extends Orig {
      constructor(u: any, p?: any) {
        super(u, p);
        this.addEventListener("message", (e: MessageEvent) => {
          w.__perf.msgs++;
          w.__perf.sizes.push(typeof e.data === "string" ? -e.data.length : (e.data as ArrayBuffer).byteLength);
          w.__perf.bytes += typeof e.data === "string" ? e.data.length : (e.data as ArrayBuffer).byteLength;
        });
      }
      send(d: any) {
        w.__perf.sent++;
        if (typeof d === "string" && d.includes("resize_split")) w.__perf.resize++;
        super.send(d);
      }
    };
    try {
      new PerformanceObserver((l) => l.getEntries().forEach((e) => w.__perf.long.push(e.duration))).observe({ entryTypes: ["longtask"] });
    } catch {
      w.__perf.noLongtask = true;
    }
    // Fallback long-task proxy (WebKit has no longtask): gaps between frames.
    let last = performance.now();
    const tick = (t: number) => {
      w.__perf.frames.push(t - last);
      last = t;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
    document.addEventListener("DOMContentLoaded", () => {
      new MutationObserver((m) => (w.__perf.mut += m.length)).observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    });
  });
}

const pct = (a: number[], p: number) => {
  if (!a.length) return null;
  const s = [...a].sort((x, y) => x - y);
  return +s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))].toFixed(1);
};

const BUSY_CMD = `while :; do clear; seq 1 200; tail -n1 ${SENTINEL} 2>/dev/null; sleep 0.05; done`;
const IDLE_CMD = `tail -n0 -F ${SENTINEL} 2>/dev/null`;

/** Fill the shown tab to PANES panes; the first BUSY run a redrawing loop. */
async function fill(page: Page, busy: number) {
  const ps = await panes(page);
  for (let i = ps.length; i < PANES; i++) {
    const cur = await panes(page);
    const from = cur[cur.length - 1];
    await page.evaluate(([pane, edge]) => window.__arugula.client.intent({ op: "split", pane, edge }), [from, i % 2 ? "bottom" : "right"] as const);
    await expect.poll(() => panes(page)).toHaveLength(i + 1);
  }
  const all = await panes(page);
  for (const p of all) await ready(page, p);
  for (let i = 0; i < all.length; i++) await page.evaluate(([p, c]) => window.__arugula.client.input(p as number, new TextEncoder().encode(c as string)), [all[i], `${i < busy ? BUSY_CMD : IDLE_CMD}\n`] as const);
  return all;
}

test("drag a divider", async ({ page }) => {
  await instrument(page);
  await reset(page);
  writeFileSync(SENTINEL, "");
  await fill(page, BUSY);
  await page.waitForTimeout(1500);
  const div = page.locator(".divider.live").first();
  const box = (await div.boundingBox())!;
  const row = ((await div.getAttribute("class")) ?? "").includes("col-resize");
  const sx = box.x + box.width / 2, sy = box.y + box.height / 2;
  await page.evaluate(() => {
    const w = window as any;
    w.__drag = { moves: [] as [number, number][], pos: [] as [number, number][] };
    const el = document.querySelector(".divider.live") as HTMLElement;
    const read = () => {
      const r = el.getBoundingClientRect();
      return Math.round(w.__row ? r.x : r.y);
    };
    w.__read = read;
    window.addEventListener("pointermove", (e) => w.__drag.moves.push([performance.now(), w.__row ? e.clientX : e.clientY]), true);
    let prev = -1;
    const tick = () => {
      const v = read();
      if (v !== prev) { w.__drag.pos.push([performance.now(), v]); prev = v; }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await page.evaluate((r) => ((window as any).__row = r), row);
  const reset0 = () => page.evaluate(() => { const p = (window as any).__perf; p.msgs = 0; p.bytes = 0; p.resize = 0; p.long.length = 0; p.mut = 0; p.frames.length = 0; });
  await reset0();
  const cdp = process.env.PERF_PROFILE ? await page.context().newCDPSession(page) : null;
  if (cdp) { await cdp.send("Profiler.enable"); await cdp.send("Profiler.setSamplingInterval", { interval: 200 }); await cdp.send("Profiler.start"); }
  await page.mouse.move(sx, sy);
  await page.mouse.down();
  const span = row ? 300 : 200;
  const steps = 120; // 2 s at ~60 Hz
  const t0 = Date.now();
  for (let i = 1; i <= steps; i++) {
    const f = i / steps;
    await page.mouse.move(row ? sx + span * f : sx, row ? sy : sy + span * f);
    const wait = t0 + (i * 2000) / steps - Date.now();
    if (wait > 0) await page.waitForTimeout(wait);
  }
  await page.waitForTimeout(500);
  await page.mouse.up();
  await page.waitForTimeout(300);
  if (cdp) {
    const { profile } = (await cdp.send("Profiler.stop")) as any;
    const self = new Map<number, number>();
    profile.samples.forEach((id: number, i: number) => self.set(id, (self.get(id) ?? 0) + (profile.timeDeltas[i] ?? 0)));
    const by = new Map<string, number>();
    for (const n of profile.nodes) {
      const f = n.callFrame;
      const k = `${f.functionName || "(anon)"} ${f.url.split("/").pop()}:${f.lineNumber + 1}`;
      by.set(k, (by.get(k) ?? 0) + (self.get(n.id) ?? 0));
    }
    const total = [...by.values()].reduce((a, b) => a + b, 0);
    const top = [...by.entries()].sort((a, b) => b[1] - a[1]).slice(0, 12).map(([k, v]) => `${(v / 1000).toFixed(0)}ms ${((100 * v) / total).toFixed(1)}% ${k}`);
    console.log("PERF " + JSON.stringify({ label: LABEL, kind: "profile", total_ms: Math.round(total / 1000), top }));
  }
  const r = await page.evaluate(() => {
    const w = window as any;
    return { perf: { ...w.__perf, frames: [...w.__perf.frames] }, drag: w.__drag };
  });
  // Latency: for each distinct divider position after the start, the time from
  // the first pointermove at (or beyond) that travel to the position on screen.
  const start = r.drag.pos[0][1];
  const lats: number[] = [];
  const startMove = r.drag.moves[0]?.[1] ?? 0;
  for (const [t, v] of r.drag.pos.slice(1)) {
    const travel = v - start;
    const m = r.drag.moves.find(([, x]: [number, number]) => x - startMove >= travel - 3);
    if (m) lats.push(t - m[0]);
  }
  const lastMove = r.drag.moves[r.drag.moves.length - 1];
  const finalPos = r.drag.pos[r.drag.pos.length - 1];
  const gaps = r.perf.frames.filter((f: number) => f > 50);
  const out = {
    label: LABEL, kind: "drag", rtt: RTT, panes: PANES, busy: BUSY,
    positions: r.drag.pos.length - 1, lat_p50: pct(lats, 50), lat_p95: pct(lats, 95), lat_max: pct(lats, 100),
    resize_intents: r.perf.resize, ws_msgs: r.perf.msgs, ws_bytes: r.perf.bytes,
    longtasks: r.perf.noLongtask ? "n/a" : r.perf.long.length, longtask_ms: r.perf.noLongtask ? "n/a" : +r.perf.long.reduce((a: number, b: number) => a + b, 0).toFixed(0),
    frame_gaps_gt50: gaps.length, frame_p50: pct(r.perf.frames, 50), frame_p95: pct(r.perf.frames, 95), frame_max: pct(r.perf.frames, 100),
    dom_mutations: r.perf.mut,
    travel_end_lag_px: lastMove && finalPos ? Math.round(lastMove[1] - startMove - (finalPos[1] - start)) : null,
  };
  console.log("PERF " + JSON.stringify(out));
});

async function tabSwitch(page: Page, busy: number, cold = false) {
  await instrument(page);
  await reset(page);
  writeFileSync(SENTINEL, "");
  // Tab 1 stays a single shell; tab 2 gets the panes.
  const tabs = await page.evaluate(async () => {
    const c = window.__arugula.client;
    c.intent({ op: "new_tab", session: c.session!, from_pane: null });
    return null;
  });
  void tabs;
  await expect.poll(() => page.evaluate(() => { const c = window.__arugula.client; return c.state!.sessions.find((s) => s.id === c.session)!.tabs.length; })).toBe(2);
  await expect.poll(() => panes(page)).toHaveLength(1);
  await ready(page, (await panes(page))[0]);
  const ids = await page.evaluate(() => { const c = window.__arugula.client; return { tabs: c.state!.sessions.find((s) => s.id === c.session)!.tabs, cur: c.tab! }; });
  const two = ids.cur;
  const one = ids.tabs.find((t) => t !== two)!;
  const all = await fill(page, busy);
  await page.waitForTimeout(1500);
  const sel = (t: number) => page.locator(`.tab[data-tab-id="${t}"]`);
  const lat: number[] = [];
  let firstSizes: number[] = [];
  const msgs: number[] = [], bytes: number[] = [];
  await sel(one).click();
  for (let i = 0; i < SWITCHES; i++) {
    await page.waitForTimeout(1500); // away on tab 1, tab 2 hidden
    if (cold) {
      // A fresh page (a window opened, a reload): nothing of tab 2 is drawn yet.
      await page.reload();
      await open(page);
      await sel(one).click();
      await page.waitForTimeout(1500);
    }
    const token = `Z${String.fromCharCode(97 + i)}Q${i}`; // short: panes are ~16 columns wide
    appendFileSync(SENTINEL, token + "\n");
    await page.waitForTimeout(600); // the daemon has printed it by now
    await page.evaluate(() => { const p = (window as any).__perf; p.msgs = 0; p.bytes = 0; p.sizes.length = 0; });
    const t0 = await page.evaluate(() => performance.now());
    await sel(two).click();
    const dt = await page.evaluate(
      ([ps, token, t0]) =>
        new Promise<number>((res) => {
          const tick = () => {
            if ((ps as number[]).every((p) => window.__arugula.text(p).includes(token as string))) res(performance.now() - (t0 as number));
            else if (performance.now() - (t0 as number) > 20000) res(-1);
            else requestAnimationFrame(tick);
          };
          tick();
        }),
      [all, token, t0] as const,
    );
    if (dt < 0 && i === 0)
      console.log("DEBUG " + JSON.stringify(await page.evaluate(([ps, token]) => (ps as number[]).map((p) => [p, window.__arugula.text(p).includes(token as string), window.__arugula.text(p).slice(-120)]), [all, token] as const)));
    lat.push(dt);
    const m = await page.evaluate(() => { const p = (window as any).__perf; return [p.msgs, p.bytes, p.sizes.slice()]; });
    if (i === 0) firstSizes = m[2];
    msgs.push(m[0]); bytes.push(m[1]);
    await page.waitForTimeout(500);
    await sel(one).click();
  }
  const ok = lat.filter((x) => x >= 0);
  const long = await page.evaluate(() => { const p = (window as any).__perf; return p.noLongtask ? null : p.long; });
  console.log("PERF " + JSON.stringify({
    label: LABEL, kind: cold ? "switch-cold" : "switch", rtt: RTT, first_switch_msg_sizes: firstSizes.slice(0, 60), panes: PANES, busy,
    p50: pct(ok, 50), p95: pct(ok, 95), max: pct(ok, 100), timeouts: lat.length - ok.length, all: lat.map((x) => +x.toFixed(0)),
    ws_msgs_first500ms_med: pct(msgs, 50), ws_bytes_med: pct(bytes, 50),
    longtasks_total: long ? long.length : "n/a", longtask_ms: long ? +long.reduce((a: number, b: number) => a + b, 0).toFixed(0) : "n/a",
  }));
}

test("tab switch, busy panes", async ({ page }) => { await tabSwitch(page, BUSY); });
test("tab switch, no busy panes", async ({ page }) => { await tabSwitch(page, 0); });
test("tab switch cold, busy panes", async ({ page }) => { await tabSwitch(page, BUSY, true); });
test("tab switch cold, no busy panes", async ({ page }) => { await tabSwitch(page, 0, true); });
