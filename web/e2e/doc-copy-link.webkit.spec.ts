// A Doc block's Copy link in WebKit (Arugula #877), the desktop app's engine.
// Copy link asks the machine for the link first. WebKit lets a click write
// the clipboard only while the click is still going, so a write after that
// request was refused and the button said "Couldn't copy". Playwright's
// WebKit doesn't hold a page to that rule, and its clipboard can't be read,
// so the page gets a clipboard that does: a write started after the click's
// task is refused, and what was written is kept for the test to read.
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ChildProcess } from "node:child_process";
import { expect, test } from "@playwright/test";
import { newDoc, type Post } from "./doc-steps";
import { closeContexts, menuItem, open } from "./helpers";
import { daemon } from "./phones";

// A daemon of its own, so the test can write its Doc's state. Its public
// link is only the entry the machine keeps for it: no control is asked.
let dir = "";
let url = "";
let proc: ChildProcess | undefined;

// Playwright's WebKit stops loading the page once the service worker is registered (platform-rail.webkit.spec.ts).
test.use({ serviceWorkers: "block", baseURL: async ({}, use) => use(url) });
test.afterAll(closeContexts);

const post: Post = async (path, body) => {
  const r = await fetch(url + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const text = await r.text();
  expect(r.ok, `${path}: ${text}`).toBe(true);
  return JSON.parse(text) as { block?: number };
};

test.describe("Copy link in WebKit", () => {
  test.beforeAll(async () => {
    dir = mkdtempSync(join(tmpdir(), "ilg-e2e-copy-link-"));
    ({ proc, url } = await daemon(join(dir, "state"), ["--tailscale-socket", "/nonexistent/sock"]));
  });
  test.afterAll(() => {
    proc?.kill("SIGKILL");
    if (dir) rmSync(dir, { recursive: true, force: true });
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
    await open(page);
    const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
    const d = await newDoc(post, term, "Copy me");
    await expect(page.locator(`[data-doc-block="${d}"] .doc-title`)).toBeVisible({ timeout: 20_000 });
    const lib = await page.evaluate((id) => (window.__arugula.client.blocks.get(id)?.state as { doc?: string }).doc!, d);
    // Shared with a link, so there's one to copy (as docs-page.spec.ts's #708 test).
    const id = "0123456789abcdef0123456789abcdef";
    const key = "k".repeat(43);
    const at = Date.now();
    const entry = { key, ver: 1, doc: lib, title: "Copy me", made: at, updated: at, size: 2048, viewer: "http://127.0.0.1:1/d" };
    writeFileSync(join(dir, "state", "doc-links.json"), JSON.stringify({ [id]: entry }));
    const link = `http://127.0.0.1:1/d/${id}#k=${key}`;
    const again = (await post("/api/blocks", { type: "doc", config: { doc: lib }, split: term, local: true })).block!;
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
