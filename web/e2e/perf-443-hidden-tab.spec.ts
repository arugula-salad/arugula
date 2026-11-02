// Measurement only (not in CI), #443: a switch to tab B (PANES idle panes showing a token)
// while another tab C, never shown during the loop, has PANES panes that are busy (or idle).
// Prints `PERF {...}` in the same shape as perf-pane-drag-tab-switch.spec.ts.
// Env: PERF_PANES (8), PERF_SWITCHES (5), PERF_LABEL, PERF_PROXY_PORT.
import { appendFileSync, writeFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { open, reset, ready, panes } from "./helpers";

const PANES = Number(process.env.PERF_PANES ?? 8);
const SWITCHES = Number(process.env.PERF_SWITCHES ?? 5);
const LABEL = process.env.PERF_LABEL ?? "run";
const SENTINEL = "/tmp/perf-443-sentinel-" + process.pid;
test.use({ viewport: { width: 1400, height: 900 } });
test.setTimeout(300_000);
if (process.env.PERF_PROXY_PORT) test.use({ baseURL: `http://127.0.0.1:${process.env.PERF_PROXY_PORT}` });

const BUSY_CMD = `while :; do clear; seq 1 200; sleep 0.05; done`;
const IDLE_CMD = `tail -n0 -F ${SENTINEL} 2>/dev/null`;
const pct = (a: number[]) => (a.length ? [...a].sort((x, y) => x - y)[Math.floor((a.length - 1) / 2)] : null);

async function fill(page: Page, cmd: string) {
  for (let i = (await panes(page)).length; i < PANES; i++) {
    const cur = await panes(page);
    await page.evaluate(([pane, edge]) => window.__arugula.client.intent({ op: "split", pane, edge }), [cur[cur.length - 1], i % 2 ? "bottom" : "right"] as const);
    await expect.poll(() => panes(page)).toHaveLength(i + 1);
  }
  const all = await panes(page);
  for (const p of all) await ready(page, p);
  for (const p of all) await page.evaluate(([p, c]) => window.__arugula.client.input(p as number, new TextEncoder().encode(c as string)), [p, `${cmd}\n`] as const);
  return all;
}

async function newTab(page: Page) {
  const n = () => page.evaluate(() => { const c = window.__arugula.client; return c.state!.sessions.find((s) => s.id === c.session)!.tabs.length; });
  const before = await n();
  await page.evaluate(() => { const c = window.__arugula.client; c.intent({ op: "new_tab", session: c.session!, from_pane: null }); });
  await expect.poll(n).toBe(before + 1);
  await expect.poll(() => panes(page)).toHaveLength(1);
  await ready(page, (await panes(page))[0]);
  return page.evaluate(() => window.__arugula.client.tab!);
}

async function run(page: Page, hiddenBusy: boolean, cold = false) {
  await page.addInitScript(() => {
    const w = window as any;
    w.__perf = { msgs: 0, bytes: 0 };
    const Orig = window.WebSocket;
    w.WebSocket = class extends Orig {
      constructor(u: any, p?: any) {
        super(u, p);
        this.addEventListener("message", (e: MessageEvent) => { w.__perf.msgs++; w.__perf.bytes += typeof e.data === "string" ? e.data.length : (e.data as ArrayBuffer).byteLength; });
      }
    };
  });
  await reset(page);
  writeFileSync(SENTINEL, "");
  const one = await page.evaluate(() => window.__arugula.client.tab!);
  const b = await newTab(page);
  const bPanes = await fill(page, IDLE_CMD);
  await newTab(page);
  await fill(page, hiddenBusy ? BUSY_CMD : IDLE_CMD);
  const sel = (t: number) => page.locator(`.tab[data-tab-id="${t}"]`);
  await sel(one).click();
  const lat: number[] = [], firsts: number[] = [], msgs: number[] = [], bytes: number[] = [];
  for (let i = 0; i < SWITCHES; i++) {
    await page.waitForTimeout(1500);
    // Cold: a fresh page, and the click on B comes the moment the page is up.
    const token = `Z${String.fromCharCode(97 + i)}Q${i}`;
    appendFileSync(SENTINEL, token + "\n");
    await page.waitForTimeout(600);
    if (cold) {
      await page.reload();
      await open(page);
      if ((await page.evaluate(() => window.__arugula.client.tab)) === b) console.log("DEBUG reload landed on B");
    }
    await page.evaluate(() => { const p = (window as any).__perf; p.msgs = 0; p.bytes = 0; (window as any).__first = undefined; });
    const t0 = await page.evaluate(() => performance.now());
    await sel(b).click();
    const dt = await page.evaluate(([ps, token, t0]) => new Promise<number>((res) => {
      const tick = () => {
        const w = window as any;
        if (w.__first === undefined && (ps as number[]).some((p) => window.__arugula.text(p).includes(token as string))) w.__first = performance.now() - (t0 as number);
        if ((ps as number[]).every((p) => window.__arugula.text(p).includes(token as string))) res(performance.now() - (t0 as number));
        else if (performance.now() - (t0 as number) > 30000) res(-1);
        else requestAnimationFrame(tick);
      };
      tick();
    }), [bPanes, token, t0] as const);
    lat.push(dt);
    firsts.push(await page.evaluate(() => (window as any).__first ?? -1));
    const m = await page.evaluate(() => { const p = (window as any).__perf; return [p.msgs, p.bytes]; });
    msgs.push(m[0]); bytes.push(m[1]);
    await page.waitForTimeout(500);
    await sel(one).click();
  }
  const ok = lat.filter((x) => x >= 0);
  console.log("PERF " + JSON.stringify({ label: LABEL, kind: cold ? "other-tab-cold" : "other-tab", busy: hiddenBusy ? PANES : 0, first_p50: pct(firsts.filter((x) => x >= 0)), p50: pct(ok), timeouts: lat.length - ok.length, all: lat.map((x) => +x.toFixed(0)), ws_msgs_first500ms_med: pct(msgs), ws_bytes_med: pct(bytes) }));
}

test("switch to idle tab, other tab busy", async ({ page }) => { await run(page, true); });
test("switch to idle tab, other tab idle", async ({ page }) => { await run(page, false); });
test("cold switch to idle tab, other tab busy", async ({ page }) => { await run(page, true, true); });
test("cold switch to idle tab, other tab idle", async ({ page }) => { await run(page, false, true); });
