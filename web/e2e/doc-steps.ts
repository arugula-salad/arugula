// #963: the Doc block's checks, run in Chrome (doc.spec.ts: a desktop and a
// Pixel 7) and WebKit (doc.webkit.spec.ts: an iPhone). An agent builds a Doc
// through the daemon (here the owner's calls, which are the agent's tool
// calls' effect); the page shows the outline with every intent, then each
// fill as it lands, with no reload.

import { expect, type Page } from "@playwright/test";
import type { PaneId } from "../src/proto";

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

const fill = (post: Post, doc: number, section: string, markdown: string) =>
  post(`/api/blocks/${doc}/call/doc_fill`, { section, markdown });

/** Show `doc` on a phone, where one block is up at a time. */
async function bring(page: Page, doc: number) {
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
