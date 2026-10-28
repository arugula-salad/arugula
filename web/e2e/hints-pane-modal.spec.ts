// A big tip (DECISIONS.md "Hints come from facts, once per account"): a
// hint with `place: "pane-modal"` covers its pane, and only its pane,
// until it's answered (×, Escape or a button). The pane keeps drawing
// underneath and takes no clicks or keys until then. It's seen when it's
// answered, not when it shows, so one hidden by a tab switch comes back.
// Its "Don't show tips like this again" turns Big tips off: every pane
// modal stops, chips carry on, and Settings › Big tips turns them back on.
// Over a terminal, an agent block and a browser block. Never on a phone,
// while someone types, over a pane someone else drives, or with Tips off.
//
// No feature has one yet (nvim's help card, #1735, will be the first), so
// this spec brings its own: `window.__arugulaTestHints`, which the hints
// read only under a test runner (hints/index.ts). Each holds while
// `window.__tipOn[kind]` names a pane.

import { spawn, type ChildProcess } from "node:child_process";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, menuItem, paneEl, panes, ready, reset, text } from "./helpers";
import type { PaneId } from "../src/shared/proto";
import { ANY, daemonPort } from "./ports";

const fake = fileURLToPath(new URL("../../crates/daemon/tests/fake_acp.py", import.meta.url));
const OWNER = "me@example.com";
const FRIEND = "friend@example.com";

let APP = "";
let state = "";
let daemon: ChildProcess | undefined;
/** The page the browser block shows: it counts the clicks and keys that reach it. */
let site: Server;
let sitePort = 0;

test.use({ baseURL: async ({}, use) => use(APP) });
test.describe.configure({ mode: "serial" });
test.afterAll(closeContexts);

type Kind = "term" | "agent" | "browser";

/** The real hints that could take the one place on screen first: said
 * already. (The daemon's Claude Code directory is the machine's, which may
 * hold conversations.) */
const SAID = { "seen:command-mark": 1, "seen:claude-conversations": 1, "seen:second-person": 1, "seen:push-device": 1 };

/** Hints on, the spec's three pane modals, and `seen` (SAID unless it
 * says otherwise). */
async function setUp(page: Page, seen: Record<string, unknown> = SAID) {
  await page.addInitScript((seen) => {
    const w = window as unknown as Record<string, unknown>;
    w.__arugulaHints = true;
    w.__tipOn = {};
    w.__arugulaTestHints = (["term", "agent", "browser"] as const).map((kind) => ({
      id: `test-tip-${kind}`,
      place: "pane-modal",
      fact: (c: { info(p: number): unknown }) => {
        const p = (w.__tipOn as Record<string, number | undefined>)[kind];
        return p !== undefined && c.info(p) ? { pane: p, args: {} } : null;
      },
      copy: () => `A tip over the ${kind}`,
      body: () => [["Press ", { kbd: "Esc" }, " to close it."], { list: ["It covers one pane", "The rest stays usable"] }],
      action: { label: "Open the palette", command: "palette.open" },
      second: { label: "Later", close: true },
    }));
    if (!localStorage.getItem("arugula.seen")) localStorage.setItem("arugula.seen", JSON.stringify(seen));
  }, seen);
}

/** Tip `kind`'s fact holds over `pane` (or no longer, with null). */
const tipOn = (page: Page, kind: Kind, pane: PaneId | null) =>
  page.evaluate(
    ({ kind, pane }) => {
      const on = (window as unknown as { __tipOn: Record<string, number | undefined> }).__tipOn;
      if (pane === null) delete on[kind];
      else on[kind] = pane;
      window.__arugula.client.emit();
    },
    { kind, pane },
  );

const tip = (page: Page, pane: PaneId) => paneEl(page, pane).locator("[data-pane-tip]");
const seenHere = (page: Page) => page.evaluate(() => JSON.parse(localStorage.getItem("arugula.seen") ?? "{}"));
const focusIn = (page: Page, sel: string) => page.evaluate((s) => !!document.activeElement?.closest(s), sel);

/** Longer than the hints hold back after a key (hints/index.ts, 1.5 s). */
const settle = (page: Page) => page.waitForTimeout(2000);

async function start() {
  daemon = spawn(
    "../target/debug/arugulad",
    [
      ...["--listen", ANY, "--block-listen", ANY, "--owner", OWNER],
      ...["--shell", "bash --norc --noprofile", "--no-manager-env", "--tailscale-socket", "/nonexistent/sock"],
    ],
    { stdio: "ignore", env: { ...process.env, ARUGULA_STATE_DIR: state } },
  );
  APP = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${APP}/api/host`)).ok) return;
    } catch {
      // not up yet
    }
    await new Promise((r) => setTimeout(r, 100));
  }
}

test.beforeAll(async () => {
  state = mkdtempSync(join(tmpdir(), "ilg-e2e-pane-modal-"));
  site = createServer((_, res) => {
    res.setHeader("content-type", "text/html");
    res.end(`<!doctype html><body style="margin:0;height:100vh"><p id="n">clicks 0 keys 0</p>
<script>let c = 0, k = 0; const say = () => (document.getElementById("n").textContent = "clicks " + c + " keys " + k);
addEventListener("pointerdown", () => { c++; say(); }); addEventListener("keydown", () => { k++; say(); });</script></body>`);
  });
  await new Promise<void>((r) => site.listen(0, "127.0.0.1", () => r()));
  sitePort = (site.address() as AddressInfo).port;
  await start();
});

test.afterAll(async () => {
  daemon?.kill("SIGKILL");
  site?.close();
  if (state) rmSync(state, { recursive: true, force: true });
});

test("over a terminal: only its pane, drawing underneath, deaf until Escape; Escape isn't the terminal's", async ({ page }) => {
  await setUp(page);
  await reset(page);
  const [term] = await panes(page);
  await page.evaluate((p) => window.__arugula.client.intent({ op: "split", pane: p, edge: "right" }), term);
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  const other = (await panes(page)).find((p) => p !== term)!;
  await ready(page, other);
  await paneEl(page, term).click({ position: { x: 40, y: 40 } });
  await settle(page);

  await tipOn(page, "term", term);
  await expect(tip(page, term)).toBeVisible();
  await expect(tip(page, term)).toContainText("A tip over the term");
  await expect(tip(page, term).locator("kbd")).toHaveText("Esc");
  await expect(tip(page, term).locator("li")).toHaveCount(2);
  await expect(tip(page, term).getByLabel("Don't show tips like this again")).not.toBeChecked();
  await expect(page.locator("[data-hint]")).toHaveCount(1);
  await expect(tip(page, other)).toHaveCount(0);
  // Inside its pane: the header and the other pane are outside it.
  const [box, slot] = [await tip(page, term).locator("..").boundingBox(), await paneEl(page, term).boundingBox()];
  expect(box!.x).toBeGreaterThanOrEqual(slot!.x - 1);
  expect(box!.y).toBeGreaterThanOrEqual(slot!.y - 1);
  expect(box!.x + box!.width).toBeLessThanOrEqual(slot!.x + slot!.width + 1);
  await expect(page.locator(`[data-pane-header="${term}"]`)).toBeVisible();
  // Seen on answer, not on show.
  expect(await seenHere(page)).not.toHaveProperty("seen:test-tip-term");
  // The focus moved in from the terminal.
  expect(await focusIn(page, "[data-pane-tip]")).toBe(true);

  // The pane keeps running and drawing underneath.
  await fetch(`${APP}/api/panes/${term}/send`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ text: "echo under-$((2+2))", enter: true }) });
  await expect.poll(() => text(page, term)).toContain("under-4");
  // But takes no keys or clicks of its own.
  await page.keyboard.type("echo LEAK1\n");
  await paneEl(page, term).click({ position: { x: 20, y: 20 } });
  await page.keyboard.type("echo LEAK2\n");
  await page.waitForTimeout(500);
  expect(await text(page, term)).not.toContain("LEAK");
  await expect(tip(page, term)).toBeVisible();

  // Another tab hides it, unanswered; back, it shows again.
  const session = await page.evaluate(() => window.__arugula.client.session!);
  const first = await page.evaluate(() => window.__arugula.client.tab!);
  await page.evaluate((s) => window.__arugula.client.intent({ op: "new_tab", session: s, from_pane: null }), session);
  await expect.poll(() => page.evaluate((t) => window.__arugula.client.tab !== t, first)).toBe(true);
  await expect(page.locator("[data-pane-tip]")).toHaveCount(0);
  expect(await seenHere(page)).not.toHaveProperty("seen:test-tip-term");
  await page.evaluate((t) => window.__arugula.client.selectTab(t), first);
  await expect(tip(page, term)).toBeVisible();

  // Escape closes it, and the terminal never hears it: an Escape before
  // `echo` would make bash read M-e, and the line would come out wrong.
  await tip(page, term).click({ position: { x: 10, y: 60 } });
  await page.keyboard.press("Escape");
  await expect(tip(page, term)).toHaveCount(0);
  expect(await seenHere(page)).toHaveProperty("seen:test-tip-term");
  await expect.poll(() => focusIn(page, `[data-pane="${term}"] .xterm`)).toBe(true);
  await page.keyboard.type("echo after-$((3+3))\n");
  await expect.poll(() => text(page, term)).toContain("after-6");
  expect(await text(page, term)).not.toContain("LEAK");

  // Answered: never again, not after a reload either.
  await page.reload();
  await expect.poll(() => page.evaluate(() => !!window.__arugula?.client.state)).toBe(true);
  await tipOn(page, "term", term);
  await settle(page);
  await expect(page.locator("[data-pane-tip]")).toHaveCount(0);
});

/** An agent block on the fake ACP server beside `term`, shown. */
async function agent(page: Page, term: PaneId): Promise<PaneId> {
  const id = await page.evaluate(
    ({ fake, term }) => window.__arugula.client.openBlock({ type: "agent", config: { agent: "acp", command: ["python3", fake], cwd: "/tmp", prompt: "hello" }, split: term, local: true } as never),
    { fake, term },
  );
  expect(id).not.toBeNull();
  await page.evaluate((id) => window.__arugula.client.setActive(id), id!);
  return id!;
}

test("over an agent block: it takes the composer's focus and gives it back; × closes it", async ({ page }) => {
  await setUp(page);
  await reset(page);
  const [term] = await panes(page);
  const block = await agent(page, term);
  const composer = paneEl(page, block).locator(".agent-composer textarea");
  await expect(composer).toBeVisible({ timeout: 20_000 });
  await composer.click();
  await settle(page);

  await tipOn(page, "agent", block);
  await expect(tip(page, block)).toBeVisible();
  await expect(composer).not.toBeFocused();
  await page.keyboard.type("LEAK");
  await composer.click({ force: true }).catch(() => {});
  await page.keyboard.type("LEAK");
  await expect(composer).toHaveValue("");

  await tip(page, block).locator("[data-hint-dismiss]").click();
  await expect(tip(page, block)).toHaveCount(0);
  await expect(composer).toBeFocused();
  expect(await seenHere(page)).toHaveProperty("seen:test-tip-agent");
  await page.keyboard.type("hi");
  await expect(composer).toHaveValue("hi");
});

/** A browser block on the spec's own page beside `term`: its id and its frame. */
async function browserBlock(page: Page, term: PaneId) {
  const id = await page.evaluate(
    ({ port, term }) => window.__arugula.client.openBlock({ type: "browser", config: { port }, split: term, local: true } as never),
    { port: sitePort, term },
  );
  expect(id).not.toBeNull();
  const frame = page.frameLocator(`[data-pane="${id}"] iframe`);
  await expect(frame.locator("#n")).toHaveText("clicks 0 keys 0", { timeout: 30_000 });
  return { id: id!, frame };
}

test("over a browser block: no clicks or keys reach the frame; Escape closes it from the frame's focus", async ({ page }) => {
  await setUp(page);
  await reset(page);
  const [term] = await panes(page);
  const { id, frame } = await browserBlock(page, term);
  // The focus inside the frame, as someone using the page has it.
  await frame.locator("body").click({ position: { x: 100, y: 100 } });
  await expect(frame.locator("#n")).toHaveText("clicks 1 keys 0");
  await settle(page);

  await tipOn(page, "browser", id);
  await expect(tip(page, id)).toBeVisible();
  expect(await focusIn(page, "[data-pane-tip]")).toBe(true);
  // Over the frame, away from the card: the tip's backdrop takes it.
  await paneEl(page, id).click({ position: { x: 8, y: 8 } });
  await page.keyboard.type("abc");
  await page.mouse.click(...(await (async () => {
    const b = (await paneEl(page, id).boundingBox())!;
    return [b.x + b.width - 8, b.y + b.height - 8] as [number, number];
  })()));
  await page.waitForTimeout(300);
  await expect(frame.locator("#n")).toHaveText("clicks 1 keys 0");

  await page.keyboard.press("Escape");
  await expect(tip(page, id)).toHaveCount(0);
  expect(await seenHere(page)).toHaveProperty("seen:test-tip-browser");
  // Answered, the frame takes them again.
  await frame.locator("body").click({ position: { x: 100, y: 100 } });
  await expect(frame.locator("#n")).toHaveText("clicks 2 keys 0");
});

test("Don't show tips like this again: every pane modal stops, chips don't; Settings › Big tips brings them back", async ({ page }) => {
  // A browser that has seen none of the three (each test's is new).
  await setUp(page);
  await reset(page);
  const [term] = await panes(page);
  const block = await agent(page, term);
  const { id: web } = await browserBlock(page, term);
  await settle(page);

  await tipOn(page, "term", term);
  await expect(tip(page, term)).toBeVisible();
  await tip(page, term).getByLabel("Don't show tips like this again").check();
  await tip(page, term).locator("[data-hint-second]").click();
  await expect(tip(page, term)).toHaveCount(0);
  expect(await seenHere(page)).toMatchObject({ "tips.modal": "off", "seen:test-tip-term": expect.any(Number) });

  // None of the three, over any pane.
  await tipOn(page, "agent", block);
  await tipOn(page, "browser", web);
  await settle(page);
  await expect(page.locator("[data-pane-tip]")).toHaveCount(0);

  // A chip still shows: the mark a command makes.
  await page.evaluate(() => {
    const s = JSON.parse(localStorage.getItem("arugula.seen") ?? "{}");
    delete s["seen:command-mark"];
    localStorage.setItem("arugula.seen", JSON.stringify(s));
  });
  await page.reload();
  await expect.poll(() => page.evaluate(() => !!window.__arugula?.client.state)).toBe(true);
  await ready(page, term);
  await tipOn(page, "agent", block);
  await tipOn(page, "browser", web);
  await fetch(`${APP}/api/panes/${term}/send`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ text: "echo mark-$((1+1))", enter: true }) });
  await expect(page.locator(`[data-pane-header="${term}"] [data-hint="command-mark"]`)).toBeVisible({ timeout: 15_000 });
  await expect(page.locator("[data-pane-tip]")).toHaveCount(0);
  await page.locator(`[data-pane-header="${term}"] [data-hint-dismiss]`).click();

  // Settings › Big tips says it's off, and turns them on again.
  await page.locator("[data-account-menu]").click();
  const big = await menuItem(page, "Big tips", { exact: true });
  await expect(big).toHaveAttribute("aria-checked", "false");
  await big.click();
  expect(await seenHere(page)).not.toHaveProperty("tips.modal");
  // The agent's, unanswered, shows; its button acts (and answers it).
  await expect(tip(page, block)).toBeVisible();
  await tip(page, block).locator("[data-hint-act]").click();
  await expect(tip(page, block)).toHaveCount(0);
  await expect(page.locator(".palette-search")).toBeVisible();
  expect(await seenHere(page)).toHaveProperty("seen:test-tip-agent");
  await page.keyboard.press("Escape");
  // Then the browser's.
  await expect(tip(page, web)).toBeVisible();
});

test("never while typing, with Tips off, over a pane someone else drives, or on a phone", async ({ page, browser }) => {
  await setUp(page);
  await reset(page);
  const [term] = await panes(page);
  await paneEl(page, term).click({ position: { x: 40, y: 40 } });

  // While typing in the page: it waits for the keys to stop.
  await page.keyboard.type("echo typing");
  await tipOn(page, "term", term);
  await page.waitForTimeout(300);
  await expect(page.locator("[data-pane-tip]")).toHaveCount(0);
  await expect(tip(page, term)).toBeVisible({ timeout: 5000 });
  await page.keyboard.press("Escape");
  await expect(tip(page, term)).toHaveCount(0);

  // Tips off: nothing, Big tips or not.
  await page.evaluate((said) => localStorage.setItem("arugula.seen", JSON.stringify({ ...said, tips: false })), SAID);
  await page.reload();
  await expect.poll(() => page.evaluate(() => !!window.__arugula?.client.state)).toBe(true);
  await tipOn(page, "agent", term);
  await settle(page);
  await expect(page.locator("[data-pane-tip]")).toHaveCount(0);

  // Someone else drives the pane (pair mode): never over it.
  await page.evaluate((said) => localStorage.setItem("arugula.seen", JSON.stringify(said)), SAID);
  const session = await page.evaluate(() => window.__arugula.client.session!);
  await fetch(`${APP}/api/acl`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ session, principal: `tailnet:${FRIEND}`, role: "editor" }) });
  const friend = await (await browser.newContext({ extraHTTPHeaders: { "tailscale-user-login": FRIEND } })).newPage();
  await friend.goto(APP);
  await expect.poll(() => friend.evaluate(() => !!window.__arugula?.client.state)).toBe(true);
  await friend.evaluate((p) => window.__arugula.client.paneOp(p, { op: "take_control" }), term);
  await page.reload();
  await expect.poll(() => page.evaluate(() => !!window.__arugula?.client.state)).toBe(true);
  await expect.poll(() => page.evaluate((p) => !!window.__arugula.client.drivenBy(p), term)).toBe(true);
  await tipOn(page, "agent", term);
  await settle(page);
  await expect(page.locator("[data-pane-tip]")).toHaveCount(0);
  await friend.context().close();

  // On a phone: no pane modals at all, over a pane of its own.
  await page.setViewportSize({ width: 390, height: 800 });
  await reset(page);
  const [mine] = await panes(page);
  expect(await page.evaluate((p) => !!window.__arugula.client.drivenBy(p), mine)).toBe(false);
  await tipOn(page, "agent", mine);
  await settle(page);
  await expect(page.locator("[data-pane-tip]")).toHaveCount(0);
  expect(await seenHere(page)).not.toHaveProperty("seen:test-tip-agent");
});
