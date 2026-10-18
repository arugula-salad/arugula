// #963 on an iPhone (WebKit, docs/testing.md "Phones"): the same checks as
// doc.spec.ts's, in the engine Safari uses. The daemon is the spec's own,
// with the `docs` flag on (labs.ts).

import { rmSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ChildProcess } from "node:child_process";
import { expect, test } from "@playwright/test";
import { fillsInPlace, type Post } from "./doc-steps";
import { open, ready } from "./helpers";
import { daemon, iphone } from "./phones";

let dir = "";
let url = "";
let proc: ChildProcess | undefined;

test.describe.configure({ mode: "serial" });
test.use({ ...iphone, baseURL: async ({}, use) => use(url) });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "ilg-e2e-doc-ios-"));
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

test("a Doc fills in on an iPhone: its outline, then each section, with no reload", async ({ page }) => {
  await open(page);
  const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, term);
  await fillsInPlace(page, post, term);
});
