// #963 on an iPhone (WebKit, docs/testing.md "Phones"): the same checks as
// doc.spec.ts's, in the engine Safari uses. The daemon is the spec's own,
// with Labs on (labs.ts). #964's too: WebKit's <img> of an SVG
// from a data: URL, and a chart drawn as SVG, which differ from Chrome's.
// #965's too: a PNG from a data: URL, replaced under its name, with no blob:.

import { rmSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { ChildProcess } from "node:child_process";
import { expect, test, type BrowserContext } from "@playwright/test";
import { attacker, bring, brokenEmbeds, protoLeavesAfterMove, protoOpenBeforeLoad, fill, fillEmbeds, prototype, prototypeRan, fillsInPlace, imagesInPlace, newDoc, seeEmbeds, type Post } from "./doc-steps";
import { open, panes, ready, reset } from "./helpers";
import { daemon, iphone } from "./phones";

let dir = "";
let url = "";
let proc: ChildProcess | undefined;

test.describe.configure({ mode: "serial" });
test.use({ ...iphone, baseURL: async ({}, use) => use(url) });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "ilg-e2e-doc-ios-"));
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
  return JSON.parse(text) as { block?: number };
};

test("a Doc fills in on an iPhone: its outline, then each section, with no reload", async ({ page }) => {
  await open(page);
  const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, term);
  await fillsInPlace(page, post, term);
});

test("a chart draws and a drawing shows through <img> in WebKit; a script in one runs nothing", async ({ page }) => {
  await open(page);
  const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, term);
  const doc = await newDoc(post, term);
  await bring(page, doc);
  await fillEmbeds(post, doc);
  await seeEmbeds(page, doc);
  await brokenEmbeds(page, post, doc);
});

test("a picture shows in WebKit, a replaced one with no reload, and other images stay text", async ({ page }) => {
  await open(page);
  const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
  await ready(page, term);
  const doc = await newDoc(post, term);
  await imagesInPlace(page, post, doc);
});

// #506: a prototype runs in WebKit's sandboxed frame (the daemon's runner page)
// and gets out of nowhere.
test("a prototype runs its script in WebKit and gets out of nowhere", async ({ page }) => {
  const a = await attacker();
  try {
    await open(page);
    const term = await page.evaluate(() => window.__arugula.client.state!.panes[0].id);
    await ready(page, term);
    const doc = await newDoc(post, term);
    await bring(page, doc);
    await fill(post, doc, "s1", prototype(a));
    await prototypeRan(page, a, "dark");
  } finally {
    a.close();
  }
});

// Arugula #869: a Doc with its prototype there before the page loads, switched
// to, on the iPhone and at a desktop's size. The Doc is made from a page of its
// own (`fresh`, in a session of its own: the earlier tests' Docs have
// prototypes too), and the page under test loads once.
const openBeforeLoad = async (context: BrowserContext) => {
  const a = await attacker();
  const fresh = async () => {
    const p = await context.newPage();
    await reset(p);
    const [term] = await panes(p);
    // Closed before the page under test opens: it checks it's the only one.
    await p.close();
    return term;
  };
  try {
    let term = await fresh();
    let page = await context.newPage();
    await protoOpenBeforeLoad(page, post, term, a);
    await page.close();
    term = await fresh();
    page = await context.newPage();
    await protoLeavesAfterMove(page, post, term);
  } finally {
    a.close();
  }
};
test("a prototype open before the page loads runs when switched to, and one that leaves is still caught", async ({ context }) => openBeforeLoad(context));
test.describe("at a desktop's size", () => {
  // Without the page's service worker: here, Playwright's WebKit never
  // finishes loading a second page it controls at this size.
  test.use({ viewport: { width: 1000, height: 640 }, isMobile: false, hasTouch: false, deviceScaleFactor: 1, userAgent: undefined, serviceWorkers: "block" });
  test("a prototype open before the page loads runs when switched to, and one that leaves is still caught", async ({ context }) => openBeforeLoad(context));
});
