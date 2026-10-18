// #963: the Doc block's checks, run in Chrome (doc.spec.ts: a desktop and a
// Pixel 7) and WebKit (doc.webkit.spec.ts: an iPhone). An agent builds a Doc
// through the daemon (here the owner's calls, which are the agent's tool
// calls' effect); the page shows the outline with every intent, then each
// fill as it lands, with no reload.

import { expect, type Page } from "@playwright/test";
import type { PaneId } from "../src/proto";
import { example } from "./doc-examples";

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

/** Show `doc` on a phone, where one block is up at a time. */
export async function bring(page: Page, doc: number) {
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
