// Right-click → Open beside → Doc… (#703): one of this machine's Docs beside
// the pane. A Doc that was closed opens as a new block beside it; one that is
// open already, even in another tab, comes forward and no second is made.
// A title that matches no Doc says so.

import type { ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { newDoc, type Post } from "./doc-steps";
import { menu, open, paneEl, panes, ready, reset, tabsInSession } from "./helpers";
import { daemon } from "./phones";

let dir = "";
let url = "";
let proc: ChildProcess | undefined;

test.describe.configure({ mode: "serial" });
test.use({ baseURL: async ({}, use) => use(url) });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-open-beside-doc-"));
  ({ proc, url } = await daemon(join(dir, "state"), ["--tailscale-socket", "/nonexistent/sock"]));
});

test.afterAll(() => {
  proc?.kill("SIGKILL");
  if (dir) rmSync(dir, { recursive: true, force: true });
});

const post: Post = async (path, body) => {
  const r = await fetch(url + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const text = await r.text();
  expect(r.ok, `${path}: ${text}`).toBe(true);
  return JSON.parse(text);
};

const docBlocks = (page: Page) => page.evaluate(() => window.__arugula.client.state!.panes.filter((p) => window.__arugula.client.info(p.id)?.type === "doc").length);
const allPanes = (page: Page) => page.evaluate(() => window.__arugula.client.state!.panes.length);
const tabOf = (page: Page, pane: number) => page.evaluate((p) => window.__arugula.client.tabOfPane(p)?.id, pane);
const active = (page: Page) => page.evaluate(() => window.__arugula.client.active()!);

/** Open beside `term` → Doc…, and answer with `typed`. */
async function pick(page: Page, term: number, typed: string) {
  await menu(page, paneEl(page, term), "Doc…");
  const ask = page.getByRole("dialog", { name: "Doc" });
  await ask.locator("input").fill(typed);
  await ask.locator("input").press("Enter");
}

test("a closed Doc opens beside the pane", async ({ page }) => {
  await open(page);
  await reset(page);
  const [term] = await panes(page);
  const d = await newDoc(post, term, "Closed one");
  await expect(page.locator(`[data-doc-block="${d}"]`)).toBeVisible();
  await post(`/api/panes/${d}/close`, {});
  await expect.poll(() => docBlocks(page)).toBe(0);

  await pick(page, term, "Closed one");
  await expect.poll(() => docBlocks(page)).toBe(1);
  const now = (await panes(page)).find((p) => p !== term)!;
  await expect(page.locator(`[data-doc-block="${now}"] .doc-title`)).toHaveText("Closed one");
  expect(await tabOf(page, now)).toBe(await tabOf(page, term));
});

test("a Doc open in another tab comes forward, and no second is made", async ({ page }) => {
  await open(page);
  await reset(page);
  const [first] = await panes(page);
  const d = await newDoc(post, first, "Elsewhere");
  await expect(page.locator(`[data-doc-block="${d}"]`)).toBeVisible();
  const home = await tabOf(page, d);

  await page.evaluate(() => {
    const c = window.__arugula.client;
    c.intent({ op: "new_tab", session: c.session!, from_pane: null });
  });
  await expect.poll(() => tabsInSession(page).then((t) => t.length)).toBe(2);
  const tabs = await tabsInSession(page);
  await page.evaluate((t) => window.__arugula.client.selectTab(t), tabs[1]);
  const [term] = await panes(page);
  await ready(page, term);
  expect(await tabOf(page, term)).not.toBe(home);
  const before = await allPanes(page);

  await pick(page, term, "Elsewhere");
  await expect.poll(() => active(page)).toBe(d);
  expect(await tabOf(page, d)).toBe(home);
  expect(await allPanes(page)).toBe(before);
  expect(await docBlocks(page)).toBe(1);
  expect([...(await panes(page))].sort()).toEqual([first, d].sort());
});

test("a title that matches no Doc says so, and opens nothing", async ({ page }) => {
  await open(page);
  await reset(page);
  const [term] = await panes(page);
  const before = await allPanes(page);
  await pick(page, term, "Nothing called this");
  await expect(page.getByText("No Doc here is called Nothing called this")).toBeVisible();
  expect(await allPanes(page)).toBe(before);
});
