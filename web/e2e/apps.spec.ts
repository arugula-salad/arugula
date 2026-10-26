// M35: an app's box as a block, framed from another site, opened by its
// address (native #206: the box signs you in with your Arugula account, so
// the daemon holds no studio login, mints no way in and keeps nothing).
//
// A fake box on `localhost`, Arugula's page on 127.0.0.1: another site, so
// the frame is third party as it is for real. Opened from a pane's menu by
// its address, the box shows in the frame at its origin; asked again, the
// dialog offers the app that's open, and picking it goes to its block. A box
// that takes the connection and never answers says so, then comes in.

import { existsSync } from "node:fs";
import { join } from "node:path";
import { createServer, type Server } from "node:http";
import { expect, test, type Page } from "@playwright/test";
import { menu, paneEl, panes, reset, closeContexts } from "./helpers";
import { listen } from "./ports";

test.afterAll(closeContexts);

let box = "";
const servers: Server[] = [];
/** Requests a hung box took and hasn't answered, to answer when it's back. */
let hung: (() => void)[] | null = null;

test.beforeAll(async () => {
  const boxServer = createServer((_req, res) => {
    const answer = () => res.writeHead(200, { "content-type": "text/html" }).end("<title>Pinboard</title><p id=app>Hello from the box</p>");
    if (hung) hung.push(answer);
    else answer();
  });
  box = `http://localhost:${await listen(boxServer)}`;
  servers.push(boxServer);
});

test.afterAll(() => {
  for (const g of hung ?? []) g();
  for (const s of servers) s.close();
});

async function blockOf(page: Page): Promise<number> {
  await expect.poll(async () => (await panes(page)).length).toBe(2);
  return page.evaluate(() => window.__arugula.client.state!.panes.find((p) => p.type === "app")!.id);
}

/** "Open an app…" from `pane`'s menu, with `address` typed. */
async function openApp(page: Page, pane: number, address: string) {
  await menu(page, paneEl(page, pane), "Open an app…");
  const dialog = page.getByRole("dialog", { name: "Open an app" });
  await dialog.getByRole("textbox").fill(address);
  await dialog.getByRole("button", { name: "OK" }).click();
}

test("an app opens by its address, framed from another site, and is offered again", async ({ page }) => {
  await reset(page);
  const term = (await panes(page))[0];
  await openApp(page, term, `${box}/somewhere?x=1`);
  const b = await blockOf(page);
  const el = paneEl(page, b);
  const host = new URL(box).host;
  await expect(el.locator(".app-name")).toHaveText(host);
  // The box's origin, and nothing else: no way in was minted for it.
  await expect(page.frameLocator(`[data-pane="${b}"] iframe`).locator("#app")).toHaveText("Hello from the box");
  expect(await el.locator("iframe").getAttribute("src")).toBe(`${box}/`);
  const s = await page.evaluate((p) => window.__arugula.client.blocks.get(p)?.state, b);
  expect(s).toEqual({ box_url: box, title: null, app: null, reloads: 0 });

  // Asked again, the dialog offers the open app; picking it goes there.
  await menu(page, paneEl(page, term), "Open an app…");
  const dialog = page.getByRole("dialog", { name: "Open an app" });
  await dialog.getByRole("group", { name: "Choices" }).getByRole("button", { name: host }).click();
  await expect(dialog).toHaveCount(0);
  expect((await panes(page)).length).toBe(2);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.active())).toBe(b);

  // The studio login is gone: no route, and no token kept.
  expect(await page.evaluate(() => window.__arugula.client.request("GET", "/api/studio").then((r) => r.status))).toBe(404);
  const state = process.env.ARUGULA_E2E_STATE;
  if (state) expect(existsSync(join(state, "studio.json"))).toBe(false);
});

test("a box that takes the connection and never answers says so, then comes in", async ({ page }) => {
  test.setTimeout(60_000);
  await reset(page);
  const term = (await panes(page))[0];
  hung = [];
  await openApp(page, term, box);
  const b = await blockOf(page);
  const el = paneEl(page, b);
  const host = new URL(box).host;

  // Not a blank white frame: a card that says why.
  await expect(el.getByText(`Loading ${host}…`)).toBeVisible();
  const stuck = el.locator("[data-app-stuck]");
  await expect(stuck).toContainText(`${host} isn't answering`, { timeout: 25_000 });
  await expect(el.locator("iframe")).toBeHidden();

  // The box answers: the frame comes in.
  const held = hung;
  hung = null;
  for (const go of held) go();
  await expect(page.frameLocator(`[data-pane="${b}"] iframe`).locator("#app")).toHaveText("Hello from the box");
  await expect(stuck).toHaveCount(0);
  await expect(el.locator("iframe")).toBeVisible();
});
