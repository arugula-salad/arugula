// A Doc block's Copy link in WebKit (Arugula #877), the desktop app's engine.
// Copy link asks the machine for the link first. WebKit lets a click write
// the clipboard only while the click is still going, so a write after that
// request was refused and the button said "Couldn't copy". Playwright's
// WebKit doesn't hold a page to that rule, and its clipboard can't be read,
// so the page gets a clipboard that does: a write started after the click's
// task is refused, and what was written is kept for the test to read.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { newDoc } from "./doc-steps";
import { DOMAIN, rig, type Rig } from "./doc-share-rig";
import { closeContexts, menuItem } from "./helpers";
import "./local-token";

// Playwright's WebKit stops loading the page once the service worker is registered (platform-rail.webkit.spec.ts).
test.use({ serviceWorkers: "block" });
test.afterAll(closeContexts);

test.describe("Copy link in WebKit", () => {
  let r: Rig;
  test.beforeAll(async () => {
    test.setTimeout(120_000);
    r = await rig(DOMAIN);
  });
  test.afterAll(async () => {
    await r?.dispose();
  });

  test("the Doc bar's Copy link copies after asking for the link (Arugula #877)", async ({ page }) => {
    await page.addInitScript(() => {
      // A click lets the clipboard be written until its task ends, as WebKit does.
      let active = false;
      addEventListener(
        "click",
        () => {
          active = true;
          setTimeout(() => (active = false), 0);
        },
        true,
      );
      const copied: string[] = [];
      (window as unknown as { copied: string[] }).copied = copied;
      const refused = () => Promise.reject(new DOMException("not in a click", "NotAllowedError"));
      const clipboard = {
        writeText: (t: string) => (active ? (copied.push(t), Promise.resolve()) : refused()),
        write: (items: ClipboardItem[]) => {
          if (!active) return refused();
          return Promise.all(items.map((i) => i.getType("text/plain").then((b) => b.text()))).then((ts) => void copied.push(...ts));
        },
      };
      Object.defineProperty(navigator, "clipboard", { value: clipboard, configurable: true });
    });
    const [{ id: term }] = (await (await fetch(`${r.mini}/api/panes`)).json()) as { id: number }[];
    const d = await newDoc(r.post, term, "Copy me");
    await page.goto(r.mini);
    await expect(page.locator(`[data-doc-block="${d}"] .doc-title`)).toBeVisible({ timeout: 20_000 });
    const lib = await page.evaluate((id) => (window.__arugula.client.blocks.get(id)?.state as { doc?: string }).doc!, d);
    // A copy on the platform, so there's a link to copy (as doc-share.spec.ts's #708 test).
    const link = "http://127.0.0.1:1/p/prj_0123/docs/pdc_00000000000000000000000000000000";
    const file = join(r.state, "docs", lib, "meta.json");
    const meta = JSON.parse(readFileSync(file, "utf8")) as Record<string, unknown>;
    meta.platform = { project: "prj_0123", id: "pdc_0", link, rev: 1, assets: {} };
    writeFileSync(file, JSON.stringify(meta));
    const again = (await r.post("/api/blocks", { type: "doc", config: { doc: lib }, split: term, local: true })).block!;
    const button = page.locator(`[data-doc-bar="${again}"] [data-doc-bar-link]`);
    await expect(button).toBeEnabled({ timeout: 20_000 });

    await button.click();
    await expect(button).toContainText("Copied");
    await expect(page.getByText(/Couldn't copy/)).toHaveCount(0);
    await expect.poll(() => page.evaluate(() => (window as unknown as { copied: string[] }).copied)).toEqual([link]);

    // The block menu's Copy link too.
    // The pane menu's button: WebKit's right-click lands in the Doc's frame.
    await page.locator(`[data-pane-more="${again}"]`).click();
    await (await menuItem(page, "Copy link", { exact: true })).click();
    await expect.poll(() => page.evaluate(() => (window as unknown as { copied: string[] }).copied)).toEqual([link, link]);
    await expect(page.getByText(/Couldn't copy/)).toHaveCount(0);
  });
});
