// #1494 on an iPhone (WebKit, docs/testing.md "Phones"): a Doc whose pane
// was closed is on the Docs page, reached from the sheet, and Open shows it
// again with its content. The same checks on a desktop are docs-page.spec.ts.

import type { ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import { fill, newDoc, type Post } from "./doc-steps";
import { open, ready, sheetTab } from "./helpers";
import { daemon, iphone } from "./phones";

let dir = "";
let url = "";
let proc: ChildProcess | undefined;

test.use({ ...iphone, baseURL: async ({}, use) => use(url) });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "ilg-e2e-docs-ios-"));
  ({ proc, url } = await daemon(join(dir, "state"), ["--tailscale-socket", "/nonexistent/sock", "--wisp-token-file", "/nonexistent"]));
});

test.afterAll(() => {
  proc?.kill("SIGKILL");
  if (dir) rmSync(dir, { recursive: true, force: true });
});

const post: Post = async (path, body) => {
  const r = await fetch(url + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const text = await r.text();
  expect(r.ok, `${path}: ${text}`).toBe(true);
  return JSON.parse(text) as { block?: number };
};

test("a closed Doc is on the Docs page on an iPhone, and Open shows it again", async ({ page }) => {
  await open(page);
  const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, term);
  const d = await newDoc(post, term, "Quarterly plan");
  await fill(post, d, "s1", "Findings ZEBRA-QUARTZ-4471");
  await post(`/api/panes/${d}/close`, {});

  await page.locator(".sheet-button").tap();
  await sheetTab(page, "More");
  await page.locator("[data-open-docs]").tap();
  const row = page.locator("[data-doc]").filter({ hasText: "Quarterly plan" });
  await expect(row).toHaveCount(1);
  await row.locator("[data-open-doc]").tap();

  await expect(page.locator("[data-docs-page]")).toHaveCount(0);
  const block = page.locator("[data-doc-block]");
  await expect(block.locator(".doc-title")).toHaveText("Quarterly plan");
  await expect(block.locator('[data-section="s1"]')).toContainText("Findings ZEBRA-QUARTZ-4471");
});
