// Hints from facts (DECISIONS.md "Hints come from facts, once per
// account"), with the port chip: a port listening in a pane is the fact;
// its pane's header says so once, with Open beside; once seen it never
// comes back, not after a reload either; × dismisses it; and with Tips off
// in Settings nothing is said until they're on again. Off control, "seen"
// is this browser's (localStorage); the account's is control's
// `/api/me/prefs` (crates/control/src/prefs.rs, its own tests).

import { spawn, type ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { closeContexts, menuItem, open, paneEl, panes, reset, text, type } from "./helpers";
import type { PaneId } from "../src/shared/proto";
import { ANY, daemonPort } from "./ports";

test.afterAll(closeContexts);

let APP = "";
let state = "";
let daemon: ChildProcess | undefined;
test.use({ baseURL: async ({}, use) => use(APP) });
test.describe.configure({ mode: "serial" });

test.beforeAll(async () => {
  state = mkdtempSync(join(tmpdir(), "ilg-e2e-hints-"));
  // Block sites, so a port can open beside its terminal.
  daemon = spawn("../target/debug/arugulad", ["--listen", ANY, "--block-listen", ANY, "--shell", "bash --norc --noprofile"], {
    stdio: "ignore",
    env: { ...process.env, ARUGULA_STATE_DIR: state },
  });
  APP = `http://127.0.0.1:${await daemonPort(state, daemon)}`;
});

test.afterAll(async () => {
  daemon?.kill("SIGKILL");
  if (state) rmSync(state, { recursive: true, force: true });
});

const chip = (page: Page, pane: PaneId) => page.locator(`[data-pane-header="${pane}"] [data-hint="port-listening"]`);
const seenHere = (page: Page) => page.evaluate(() => JSON.parse(localStorage.getItem("arugula.seen") ?? "{}"));

/** A dev server in `pane`, on a port of its own choosing: that port. */
async function serve(page: Page, pane: PaneId): Promise<number> {
  await type(page, pane, "python3 -m http.server 0 --bind 127.0.0.1\n");
  let port = 0;
  await expect
    .poll(async () => {
      port = Number(/Serving HTTP on \S+ port (\d+)/.exec(await text(page, pane))?.[1] ?? 0);
      return port;
    })
    .toBeGreaterThan(0);
  return port;
}

/** Long enough for the hints to have asked about the pane's ports, more
 * than once (hints/ports.ts asks at most every 2 s a pane). */
const quiet = (page: Page) => page.waitForTimeout(4500);

test("a port listening in a pane: a chip in its header, Open beside opens it, and only once", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const port = await serve(page, term);

  await expect(chip(page, term)).toContainText(`:${port} is listening`);
  await expect(page.locator("[data-hint]")).toHaveCount(1);
  expect(await seenHere(page)).toHaveProperty("seen:port-listening");

  await chip(page, term).locator("[data-hint-act]").click();
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  const block = (await panes(page)).find((p) => p !== term)!;
  expect(await page.evaluate((b) => window.__arugula.client.info(b)?.type, block)).toBe("browser");
  await expect(paneEl(page, block).locator(".browser-url input")).toHaveValue(`:${port}/`);
  await expect(chip(page, term)).toHaveCount(0);

  // The server still listens, but it's been said.
  await page.reload();
  await open(page);
  await quiet(page);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  await page.evaluate((p) => window.__arugula.client.intent({ op: "close_pane", pane: p }), block);
});

test("Tips off says nothing; turned on, the chip comes; × dismisses it for good", async ({ page }) => {
  // A browser that turned Tips off and hasn't seen the port chip.
  await page.addInitScript(() => {
    if (!sessionStorage.getItem("hints-spec")) {
      localStorage.setItem("arugula.seen", JSON.stringify({ tips: false }));
      sessionStorage.setItem("hints-spec", "1");
    }
  });
  await reset(page);
  const [term] = await panes(page);
  await serve(page, term);
  await quiet(page);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  expect(await seenHere(page)).toEqual({ tips: false });

  // Settings › Tips.
  await page.locator("[data-account-menu]").click();
  const tips = await menuItem(page, "Tips", { exact: true });
  await expect(tips).toHaveAttribute("aria-checked", "false");
  await tips.click();
  await expect(chip(page, term)).toBeVisible();
  expect(await seenHere(page)).not.toHaveProperty("tips");

  await chip(page, term).locator("[data-hint-dismiss]").click();
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  expect(await seenHere(page)).toHaveProperty("seen:port-listening");
  await quiet(page);
  await expect(page.locator("[data-hint]")).toHaveCount(0);
  expect((await panes(page)).length).toBe(1);
});
