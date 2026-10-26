// #963: the Doc block's checks, run in Chrome (doc.spec.ts: a desktop and a
// Pixel 7) and WebKit (doc.webkit.spec.ts: an iPhone). An agent builds a Doc
// through the daemon (here the owner's calls, which are the agent's tool
// calls' effect); the page shows the outline with every intent, then each
// fill as it lands, with no reload.

import { deflateSync } from "node:zlib";
import { expect, type Page } from "@playwright/test";
import type { PaneId } from "../src/shared/proto";
import { example } from "./doc-examples";

declare global {
  interface Window {
    /** The Doc view's pictures, by `name/rev` (doc.tsx); gone with the view. */
    __arugulaDocImages?: (id: number) => Map<string, string> | undefined;
  }
}

/** A POST to the daemon: the body it answered. */
export type Post = (path: string, body: unknown) => Promise<{ block?: number }>;

const SECTIONS = [
  { heading: "Goals", intent: "the three outcomes we commit to" },
  { heading: "Plan", intent: "steps and dates" },
  { heading: "Risks", intent: "what could go wrong" },
];

/** Open a Doc beside `term` and give it its outline. */
export async function newDoc(post: Post, term: PaneId, title = "Quarterly plan"): Promise<number> {
  const made = await post("/api/blocks", { type: "doc", config: { title }, split: term, local: true });
  const doc = made.block!;
  await post(`/api/blocks/${doc}/call/doc_outline`, { title, sections: SECTIONS });
  return doc;
}

export const fill = (post: Post, doc: number, section: string, markdown: string) =>
  post(`/api/blocks/${doc}/call/doc_fill`, { section, markdown });

/** Show `doc` on a phone, where one block is up at a time. A block opened a
 * moment ago isn't in the client's state yet, and `setActive` ignores a pane
 * it doesn't know (the race `Client.showWhenKnown` is for), so wait for it. */
export async function bring(page: Page, doc: number) {
  await page.waitForFunction((d) => !!window.__arugula.client.info(d), doc);
  await page.evaluate((d) => window.__arugula.client.setActive(d), doc);
}

/** The outline appears with every intent; each fill shows its text with no
 * reload; a refill replaces it; a script tag or a `javascript:` link in a
 * body stays text. */
export async function fillsInPlace(page: Page, post: Post, term: PaneId) {
  const doc = await newDoc(post, term);
  await bring(page, doc);
  const block = page.locator(`[data-doc-block="${doc}"]`);
  await expect(block.locator(".doc-title")).toHaveText("Quarterly plan", { timeout: 15_000 });
  const section = (id: string) => block.locator(`[data-section="${id}"]`);

  // Every section is there, pending, with what it will hold.
  await expect(block.locator("[data-section]")).toHaveCount(3);
  for (const [i, s] of SECTIONS.entries()) {
    const row = section(`s${i + 1}`);
    await expect(row).toHaveAttribute("data-pending", "");
    await expect(row.locator(".doc-heading")).toHaveText(s.heading);
    await expect(row).toContainText("Coming…");
    await expect(row).toContainText(s.intent);
  }
  await expect(block.locator("[data-doc-progress]")).toHaveText("0 of 3 sections written");

  // A fill shows its text, in place: the page is the one we had.
  await page.evaluate(() => ((window as unknown as { __same: boolean }).__same = true));
  await fill(post, doc, "s1", "We will **ship** the Doc block.\n\n- one\n- two");
  await expect(section("s1")).not.toHaveAttribute("data-pending", "");
  await expect(section("s1").locator("strong")).toHaveText("ship");
  await expect(section("s1").locator("li")).toHaveCount(2);
  await expect(section("s1").locator(".doc-fresh")).toHaveCount(1);
  await expect(section("s2")).toHaveAttribute("data-pending", "");
  await expect(block.locator("[data-doc-progress]")).toHaveText("1 of 3 sections written");

  // A body is text and Markdown, never HTML or a script.
  await page.evaluate(() => delete (window as unknown as { __pwned?: number }).__pwned);
  await fill(post, doc, "s2", "<script>window.__pwned = 1</script>\n\n[click](javascript:window.__pwned=2) and [ok](https://example.com/x)");
  await expect(section("s2")).not.toHaveAttribute("data-pending", "");
  await expect(section("s2")).toContainText("<script>window.__pwned = 1</script>");
  await expect(block.locator("script")).toHaveCount(0);
  await expect(block.locator('a[href^="javascript"]')).toHaveCount(0);
  await expect(section("s2").locator('a[href="https://example.com/x"]')).toHaveCount(1);
  expect(await page.evaluate(() => (window as unknown as { __pwned?: number }).__pwned)).toBeUndefined();

  // Refilling replaces the text, and the highlight plays again on it.
  await fill(post, doc, "s1", "Revised goals.");
  await expect(section("s1")).toContainText("Revised goals.");
  await expect(section("s1")).not.toContainText("ship");
  await expect(section("s1").locator(".doc-fresh")).toHaveCount(1);

  await fill(post, doc, "s3", "Few risks.");
  await expect(block.locator("[data-doc-progress]")).toHaveText("3 of 3 sections written");
  await expect(block.locator("[data-pending]")).toHaveCount(0);
  // Nothing reloaded the page, and the viewer was never scrolled.
  expect(await page.evaluate(() => (window as unknown as { __same?: boolean }).__same)).toBe(true);
  expect(await block.evaluate((el) => el.scrollTop)).toBe(0);
  return doc;
}

const pwned = (page: Page) => page.evaluate(() => (window as unknown as { __pwned?: number }).__pwned);

/** A chart in s1, a drawing in s2 and one with a script in it in s3 (the
 * grammar's own examples), as the agent sends them. */
export async function fillEmbeds(post: Post, doc: number) {
  await fill(post, doc, "s1", example("bar-chart.md"));
  await fill(post, doc, "s2", example("drawing.md"));
  await fill(post, doc, "s3", example("script-drawing.md"));
}

/** What `fillEmbeds` shows, on `page`: the chart is SVG with a bar a row,
 * the drawings are pictures (an <img> that loaded, a data: URL, no SVG
 * node of theirs in the page), and the script in one never ran. */
export async function seeEmbeds(page: Page, doc: number) {
  const block = page.locator(`[data-doc-block="${doc}"]`);
  const section = (id: string) => block.locator(`[data-section="${id}"]`);
  const chart = section("s1").locator("svg.chart");
  await expect(chart).toBeVisible({ timeout: 15_000 });
  await expect(chart.locator("rect[data-bar]")).toHaveCount(3);
  await expect(chart).toHaveAttribute("aria-label", "Sales by month, bar chart. sales: Jan 12, Feb 18, Mar 25");
  await expect(chart.locator("text", { hasText: "Mar" })).toHaveCount(1);
  await expect(section("s1")).toContainText("Up and to the right.");
  await expect(section("s1").locator("[data-chart-note]")).toHaveCount(0);
  // Heights are in proportion to the values from zero.
  const heights = await chart.locator("rect[data-bar]").evaluateAll((rs) => rs.map((r) => Number(r.getAttribute("height"))));
  expect(heights[0] / heights[2]).toBeCloseTo(12 / 25, 6);

  const loaded = (id: string) =>
    section(id)
      .locator("img.doc-drawing")
      .evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth > 0);
  for (const id of ["s2", "s3"]) {
    const img = section(id).locator("img.doc-drawing");
    await expect(img).toBeVisible();
    await expect.poll(() => loaded(id)).toBe(true);
    await expect(img).toHaveAttribute("src", /^data:image\/svg\+xml;charset=utf-8,%3Csvg/);
    // Only ever a picture: nothing of the drawing is in the page.
    await expect(section(id).locator("svg, script, rect, text")).toHaveCount(0);
  }
  await expect(section("s2").locator("img")).toHaveAttribute("alt", "Idea to app");
  await expect(section("s3").locator("img")).toHaveAttribute("alt", "drawing");
  // The script in s3's drawing ran nowhere (it is an <img>), nor did its onload.
  await page.waitForTimeout(300);
  expect(await pwned(page)).toBeUndefined();
}

/** Fences sit where the daemon looks for them: one in a list item draws
 * (and the daemon checks it); one in a quote is code, on both sides. */
export async function placedEmbeds(page: Page, post: Post, doc: number) {
  const block = page.locator(`[data-doc-block="${doc}"]`);
  const section = (id: string) => block.locator(`[data-section="${id}"]`);
  const chart = example("bar-chart.md").split("```")[1].replace(/^chart\n/, "");
  await fill(post, doc, "s1", `- item\n\n  \`\`\`chart\n  ${chart.trim()}\n  \`\`\`\n`);
  await expect(section("s1").locator("li svg.chart rect[data-bar]")).toHaveCount(3, { timeout: 15_000 });
  await fill(post, doc, "s1", example("quoted-chart.md"));
  await expect(section("s1").locator("blockquote pre")).toContainText('"type": "pie"', { timeout: 15_000 });
  await expect(section("s1").locator("svg, [data-chart-note]")).toHaveCount(0);
}

/** A drawing the browser can't read as an image says so, with its source,
 * and is never a broken picture. The daemon only checks its root, so this
 * is a legitimate fill. */
export async function brokenEmbeds(page: Page, post: Post, doc: number) {
  const section = (id: string) => page.locator(`[data-doc-block="${doc}"] [data-section="${id}"]`);
  await fill(post, doc, "s2", '```svg\n<svg xmlns="http://www.w3.org/2000/svg"><rect></svg>\n```');
  const drawn = section("s2").locator("[data-drawing-note]");
  await expect(drawn).toContainText("Drawing not drawn: the browser couldn't read it", { timeout: 15_000 });
  await expect(drawn.locator("pre")).toContainText("<rect>");
  await expect(section("s2").locator("img")).toHaveCount(0);
}

// #965: a Doc's pictures. No image file is committed (AGENTS.md "Binaries"):
// a PNG is made here, of a size that says which one it is.

const CRC = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (b: Buffer) => {
  let c = 0xffffffff;
  for (const x of b) c = CRC[(c ^ x) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
function chunk(type: string, data: Buffer): Buffer {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([len, body, crc]);
}

/** A PNG `w`×`h` pixels of one colour. */
export function png(w: number, h: number, rgb: [number, number, number] = [200, 40, 40]): Buffer {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0);
  ihdr.writeUInt32BE(h, 4);
  ihdr[8] = 8; // bits
  ihdr[9] = 2; // RGB
  const row = Buffer.from([0, ...Array.from({ length: w }, () => rgb).flat()]);
  const raw = Buffer.concat(Array.from({ length: h }, () => row));
  const magic = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  return Buffer.concat([magic, chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}

/** The owner (the agent's tool call) puts a picture in the Doc. */
export const putAsset = (post: Post, doc: number, name: string, bytes: Buffer) =>
  post(`/api/blocks/${doc}/call/doc_asset_put`, { name, data: bytes.toString("base64") });

/** The images of a section that have loaded, by their natural width. */
export const widths = (page: Page, doc: number, id: string) =>
  page
    .locator(`[data-doc-block="${doc}"] [data-section="${id}"] img.doc-image`)
    .evaluateAll((imgs) => imgs.map((i) => (i as HTMLImageElement).naturalWidth));

/** What the page asks the daemon for a picture with (`doc_asset` calls). */
export function watchAssetCalls(page: Page): string[] {
  const calls: string[] = [];
  page.on("request", (r) => {
    if (r.method() === "POST" && r.url().includes("/call/doc_asset")) calls.push(r.postData() ?? "");
  });
  return calls;
}

/** Count the blob: URLs the page makes (a Doc's pictures make none). */
export const watchBlobs = (page: Page) =>
  page.evaluate(() => {
    const w = window as unknown as { __blobs?: number };
    w.__blobs = 0;
    const make = URL.createObjectURL.bind(URL);
    URL.createObjectURL = (o: Blob | MediaSource) => {
      w.__blobs!++;
      return make(o);
    };
  });
const blobs = (page: Page) => page.evaluate(() => (window as unknown as { __blobs?: number }).__blobs);

/** A picture in a section shows (a PNG that loaded, from a data: URL, with
 * its alt text), the same name written again shows the new one with no
 * reload and one more fetch, an `http://`, `data:` or `../` image is no
 * picture and is never fetched, and closing the block empties its cache.
 * Pictures come from `doc_asset`, a call: the fetches are counted. */
export async function imagesInPlace(page: Page, post: Post, doc: number) {
  const block = page.locator(`[data-doc-block="${doc}"]`);
  const section = (id: string) => block.locator(`[data-section="${id}"]`);
  await bring(page, doc);
  await expect(block.locator(".doc-title")).toBeVisible({ timeout: 15_000 });
  const calls = watchAssetCalls(page);
  await watchBlobs(page);
  await page.evaluate(() => ((window as unknown as { __same: boolean }).__same = true));

  // The first picture: 2×2 pixels. Twice in a section, fetched once.
  await putAsset(post, doc, "flow.png", png(2, 2));
  await fill(post, doc, "s1", "The flow:\n\n![The flow, in boxes](assets/flow.png)\n\nAnd again: ![Again](assets/flow.png)");
  const imgs = section("s1").locator("img.doc-image");
  await expect(imgs).toHaveCount(2, { timeout: 15_000 });
  await expect.poll(() => widths(page, doc, "s1")).toEqual([2, 2]);
  await expect(imgs.first()).toHaveAttribute("alt", "The flow, in boxes");
  await expect(imgs.first()).toHaveAttribute("loading", "lazy");
  await expect(imgs.first()).toHaveAttribute("src", /^data:image\/png;base64,/);
  expect(calls).toEqual([JSON.stringify({ name: "flow.png" })]);
  // The text of the Doc names them.
  expect(await page.evaluate((d) => window.__arugula.text(d), doc)).toContain("[image: The flow, in boxes]");

  // Written again under the same name: 4×3, shown with no reload, one more fetch.
  await putAsset(post, doc, "flow.png", png(4, 3, [40, 40, 200]));
  await expect.poll(() => widths(page, doc, "s1")).toEqual([4, 4]);
  expect(calls).toHaveLength(2);
  expect(await page.evaluate(() => (window as unknown as { __same?: boolean }).__same)).toBe(true);
  // The older rev is dropped: one picture is held.
  expect(await page.evaluate((d) => window.__arugulaDocImages?.(d)?.size, doc)).toBe(1);

  // A picture the state doesn't list shows its alt text, and no other
  // image target is a picture or is fetched. (The daemon refuses to store
  // these; the page is given them as state, to show it would not draw them.)
  const asked: string[] = [];
  await page.route(/example\.com|\/etc\/passwd/, (r) => {
    asked.push(r.request().url());
    return r.abort();
  });
  await page.evaluate(
    ([d, body]) => {
      const view = window.__arugula.client.blocks.get(d)!.view;
      view.update({
        title: "Q",
        entries: [{ id: "s1", heading: "H", intent: "", body, rev: 99 }],
        assets: [{ name: "flow.png", mime: "image/png", size: 1, rev: 1 }],
        rev: 99,
      });
    },
    [doc, "![far](http://example.com/a.png) ![inline](data:image/png;base64,AAAA) ![up](../../etc/passwd) ![gone](assets/gone.png)"] as [number, string],
  );
  await expect(section("s1")).toContainText("![inline](data:image/png;base64,AAAA)");
  await expect(section("s1")).toContainText("![up](../../etc/passwd)");
  await expect(section("s1").locator("img")).toHaveCount(0);
  await expect(section("s1").locator("[data-doc-image-missing]")).toHaveText("gone");
  await page.waitForTimeout(300);
  expect(asked).toEqual([]);
  expect(calls).toHaveLength(2);

  // Back to the daemon's own state: the picture again (from the cache).
  await fill(post, doc, "s1", "Back: ![Back](assets/flow.png)");
  await expect.poll(() => widths(page, doc, "s1")).toEqual([4]);
  expect(calls).toHaveLength(2);

  // No blob: URL was made, and closing the block empties its cache.
  expect(await blobs(page)).toBe(0);
  await page.evaluate((d) => ((window as unknown as { __kept?: Map<string, string> }).__kept = window.__arugulaDocImages?.(d)), doc);
  expect(await page.evaluate(() => (window as unknown as { __kept?: Map<string, string> }).__kept?.size)).toBe(1);
  await post(`/api/panes/${doc}/close`, {});
  await expect(block).toHaveCount(0, { timeout: 15_000 });
  expect(await page.evaluate(() => (window as unknown as { __kept?: Map<string, string> }).__kept?.size)).toBe(0);
  expect(await page.evaluate((d) => window.__arugulaDocImages?.(d), doc)).toBeUndefined();
  expect(await blobs(page)).toBe(0);
}
