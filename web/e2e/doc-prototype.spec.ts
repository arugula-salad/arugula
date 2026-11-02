// #506: a ```prototype section in a Doc runs its own script in a sandboxed frame
// and can't get out. Here, in the Doc block on the daemon's own app, which
// has no script CSP, so the frame is the host page by `srcdoc`. The shared
// viewer and the platform page, whose CSP would stop a srcdoc frame's
// scripts, are in doc-share.spec.ts and docs-platform.spec.ts; WebKit's are
// doc.webkit.spec.ts and doc-share.webkit.spec.ts.
//
// What it must do: write into its own DOM, in the Doc's colour scheme. What it
// must not: reach the page above (`parent.document`), move it (`top.location`),
// open a window, read a cookie or storage, or ask the network for anything.
// The checks are on effects: nothing reached the server the page names.

import { readFileSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import { attacker, fill, newDoc, prototype, prototypeRan, type Post } from "./doc-steps";
import { panes, reset } from "./helpers";
import { pixel7 } from "./phones";

const post = (page: Page): Post => async (path, data) => {
  const r = await page.request.post(path, { data });
  const text = await r.text();
  expect(r.ok(), `${path}: ${text}`).toBe(true);
  return JSON.parse(text);
};

test("a prototype runs its own script in a sandboxed frame and gets out of nowhere, light and dark", async ({ page }) => {
  const a = await attacker();
  try {
    await reset(page);
    const [term] = await panes(page);
    const doc = await newDoc(post(page), term);
    await fill(post(page), doc, "s1", `Here it is:\n\n${prototype(a)}\nAfter it.`);
    await expect(page.locator(`[data-doc-block="${doc}"] [data-section=s1]`)).toContainText("After it.");
    // By srcdoc: this page has no script CSP, so no other origin is needed.
    await expect(page.locator("[data-prototype-frame]")).toHaveAttribute("srcdoc", /arugula\.html/);
    await expect(page.locator("[data-prototype-frame]")).not.toHaveAttribute("src", /.*/);
    await prototypeRan(page, a, "dark");

    await page.evaluate(() => localStorage.setItem("arugula-appearance", JSON.stringify({ palette: "latte", finish: "flat", corners: "square" })));
    await page.reload();
    await expect.poll(() => page.evaluate(() => window.__arugula?.client.state !== null)).toBe(true);
    await prototypeRan(page, a, "light");
    expect(a.hits).toEqual([]);
  } finally {
    a.close();
  }
});

test("a prototype is titled from its fence line, and what the checker refuses isn't written", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fill(post(page), doc, "s1", "```prototype height=300 Views motion\n<p>hi</p>\n```");
  await expect(page.locator(`[data-doc-block="${doc}"] [data-prototype-frame]`)).toHaveAttribute("title", "Views motion");
  // A page that would fetch something is refused, in one line saying what to do.
  for (const [html, says] of [
    ['<script src="https://cdn.example/x.js"></script>', "prototype: <script src> can't load anything: inline it"],
    ['<link rel="stylesheet" href="x.css">', "prototype: <link href> can't load anything: inline it"],
    ["<iframe src=x></iframe>", "prototype: <iframe> isn't allowed"],
    ["<p>a</p><html><html>", "prototype: one document only"],
  ]) {
    const r = await page.request.post(`/api/blocks/${doc}/call/doc_fill`, { data: { section: "s2", markdown: `\`\`\`prototype\n${html}\n\`\`\`` } });
    expect(r.ok(), html).toBe(false);
    expect(await r.text(), html).toContain(says);
  }
});

test("a prototype that navigates itself away is replaced by a note with a Reload button", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fill(post(page), doc, "s1", "```prototype Wanderer\n<p id=here>here</p><script>setTimeout(() => { location.href = 'about:blank'; }, 4000)</script>\n```");
  const frame = page.locator("[data-prototype-frame]");
  await expect(frame).toHaveCount(1);
  await expect(page.frameLocator("[data-prototype-frame]").locator("#here")).toHaveText("here", { timeout: 15_000 });
  await expect(page.locator("[data-prototype-left]")).toHaveText(/This prototype tried to leave the page\. Reload it\.\s*Reload/, { timeout: 15_000 });
  await expect(frame).toHaveCount(0);
  await page.locator("[data-prototype-reload]").click();
  await expect(page.locator("[data-prototype-left]")).toHaveCount(0);
  await expect(page.frameLocator("[data-prototype-frame]").locator("#here")).toHaveText("here", { timeout: 15_000 });
  // The ordinary prototype never trips it: loads of its own are the runner's two.
  await fill(post(page), doc, "s1", "```prototype Stays\n<p id=here>stays</p>\n```");
  await expect(page.frameLocator("[data-prototype-frame]").locator("#here")).toHaveText("stays", { timeout: 15_000 });
  await page.waitForTimeout(1500);
  await expect(page.locator("[data-prototype-left]")).toHaveCount(0);
});

test("an html fence stays code: nothing framed, nothing run", async ({ page }) => {
  await reset(page);
  const [term] = await panes(page);
  const doc = await newDoc(post(page), term);
  await fill(post(page), doc, "s1", "```html\n<script src=\"https://cdn.example/x.js\"></script><iframe src=x></iframe>\n```");
  const s1 = page.locator(`[data-doc-block="${doc}"] [data-section=s1]`);
  await expect(s1.locator("pre")).toContainText("<iframe src=x></iframe>");
  await expect(s1.locator("iframe, [data-prototype-note], [data-prototype-run]")).toHaveCount(0);
});

test("the runner takes its page from its parent only, and only once", async ({ page }) => {
  await reset(page);
  const host = readFileSync(new URL("../src/shared/ui/runner.html", import.meta.url), "utf8");
  // Two sandboxed frames, as the Doc makes them: the host, and a sibling that
  // tries to hand the host a page of its own before the parent does.
  await page.evaluate((h) => {
    const mk = (name: string, srcdoc: string) => {
      const f = document.createElement("iframe");
      f.name = name;
      f.setAttribute("sandbox", "allow-scripts");
      f.srcdoc = srcdoc;
      document.body.append(f);
      return f;
    };
    const w = window as unknown as { __host: HTMLIFrameElement };
    w.__host = mk("host", h);
    mk("sibling", `<script>setTimeout(() => parent.frames[0].postMessage({ type: "arugula.html", html: "<p id=spoof>spoofed</p>", colorScheme: "dark" }, "*"), 300)</script>`);
  }, host);
  const frame = page.frameLocator("iframe[name=host]");
  await expect(frame.locator("script")).toHaveCount(1);
  await page.waitForTimeout(1500);
  await expect(frame.locator("#spoof"), "a sibling's page isn't taken").toHaveCount(0);
  await expect(frame.locator("script"), "the host is still the host").toHaveCount(1);

  // Its parent's is, once: a second is ignored (the page is the prototype's by then).
  const send = (html: string, colorScheme: string) =>
    page.evaluate(([html, colorScheme]) => (window as unknown as { __host: HTMLIFrameElement }).__host.contentWindow!.postMessage({ type: "arugula.html", html, colorScheme }, "*"), [html, colorScheme]);
  await send("<p id=first>first</p><script>document.title = 'ran'</script>", "dark");
  await expect(frame.locator("#first")).toHaveText("first");
  expect(await frame.locator("html").evaluate((el) => getComputedStyle(el).colorScheme)).toBe("dark");
  expect(await frame.locator("html").evaluate(() => document.title)).toBe("ran");
  await send("<p id=second>second</p>", "light");
  await page.waitForTimeout(300);
  await expect(frame.locator("#second")).toHaveCount(0);
  await expect(frame.locator("#first")).toHaveCount(1);
});

test.describe("on a Pixel 7", () => {
  test.use(pixel7);
  test("the prototype runs there too, and fits the phone", async ({ page }) => {
    const a = await attacker();
    try {
      await reset(page);
      const [term] = await panes(page);
      const doc = await newDoc(post(page), term);
      await page.evaluate((d) => window.__arugula.client.setActive(d), doc);
        await fill(post(page), doc, "s1", prototype(a));
      await prototypeRan(page, a, "dark");
      const width = await page.evaluate(() => document.documentElement.clientWidth);
      expect((await page.locator("[data-prototype-frame]").boundingBox())!.width).toBeLessThanOrEqual(width);
    } finally {
      a.close();
    }
  });
});
