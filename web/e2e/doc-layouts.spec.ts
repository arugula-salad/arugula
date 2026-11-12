// Doc layouts in the views that draw a Doc on a machine (Arugula #829, #830):
// a `tabs` Doc shows its sections as tabs (an ARIA tablist with arrow keys,
// Home and End, a pending section's tab greyed, ids that carry the Doc's key
// and an address that isn't the app's route touched), and a `prototype` Doc
// is its title, one line and a frame that fills the view and follows its
// resizes. The block, the Docs page's rows and its preview. The platform's
// Doc page is in docs-platform.spec.ts and the share viewer in
// doc-share.spec.ts, which have a control.

import type { ChildProcess } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { fill, type Post } from "./doc-steps";
import { panes, reset } from "./helpers";
import { daemon } from "./phones";

let dir = "";
let url = "";
let proc: ChildProcess | undefined;

test.describe.configure({ mode: "serial" });
test.use({ baseURL: async ({}, use) => use(url) });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-doc-layouts-"));
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
  return JSON.parse(text);
};

const PROTO = (title: string) => `\`\`\`prototype height=300 ${title}
<!doctype html>
<html><body style="margin:0"><p id="out">ran</p></body></html>
\`\`\`
`;

/** A Doc beside the first pane, laid out as `layout`, with the sections given. */
async function made(page: Page, title: string, layout: string, sections: { id?: string; heading: string; intent: string }[]): Promise<number> {
  const [term] = await panes(page);
  const d = (await post("/api/blocks", { type: "doc", config: { title }, split: term, local: true })).block!;
  await post(`/api/blocks/${d}/call/doc_outline`, { title, sections, layout });
  return d;
}

const TABS = [
  { heading: "Overview", intent: "what we found" },
  { heading: "Corner tuner", intent: "the radius, by hand" },
  { heading: "Motion", intent: "how it moves" },
];

/** A tabs Doc with its first two sections written and the third pending. */
async function tabsDoc(page: Page, title: string) {
  const d = await made(page, title, "tabs", TABS);
  await fill(post, d, "s1", "The overview text.");
  await fill(post, d, "s2", `The tuner text.\n\n${PROTO("Tuner")}`);
  const block = page.locator(`[data-doc-block="${d}"]`);
  await expect(block.locator(".doc-title")).toHaveText(title, { timeout: 15_000 });
  return { d, block };
}

test("a tabs Doc in the block opens on its first tab, and a click and the arrow keys, Home and End move between tabs", async ({ page }) => {
  await reset(page);
  // Wide enough that the pane is past the text's column.
  await page.setViewportSize({ width: 1700, height: 900 });
  const { block } = await tabsDoc(page, "Three ways");
  expect(await page.evaluate(() => location.hash)).toBe("");

  const list = block.getByRole("tablist");
  const tabs = list.getByRole("tab");
  await expect(tabs).toHaveText(["Overview", "Corner tuner", "Motion"]);
  await expect(tabs.nth(0)).toHaveAttribute("aria-selected", "true");
  await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "false");
  // Roving tabindex: only the chosen tab is in the tab order.
  await expect(tabs.nth(0)).toHaveAttribute("tabindex", "0");
  await expect(tabs.nth(1)).toHaveAttribute("tabindex", "-1");
  // One panel shows, the first, labelled by its tab, and the headings aren't drawn again in it.
  const panel = block.getByRole("tabpanel");
  await expect(panel).toHaveCount(1);
  await expect(panel).toContainText("The overview text.");
  await expect(block.locator(".doc-heading")).toHaveCount(0);
  const first = await tabs.nth(0).getAttribute("aria-controls");
  expect(await panel.getAttribute("id")).toBe(first);
  expect(await panel.getAttribute("aria-labelledby")).toBe(await tabs.nth(0).getAttribute("id"));
  // A section still to come: its heading greyed out, its intent as the panel.
  await expect(tabs.nth(2)).toHaveAttribute("data-pending", "");
  expect(await tabs.nth(2).evaluate((t) => getComputedStyle(t).fontStyle)).toBe("italic");

  // A prototype in a tab is as wide as the panel; text keeps the column.
  await tabs.nth(1).click();
  await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(panel).toContainText("The tuner text.");
  await expect(panel.locator("[data-prototype-frame]")).toBeVisible({ timeout: 15_000 });
  const [wide, wideBox, capped, free] = await Promise.all([
    panel.locator("[data-prototype-box]").evaluate((e) => e.getBoundingClientRect().width),
    panel.evaluate((e) => e.getBoundingClientRect().width),
    panel.locator("p").first().evaluate((e) => getComputedStyle(e).maxWidth),
    panel.locator("[data-prototype-box]").evaluate((e) => getComputedStyle(e).maxWidth),
  ]);
  expect(Math.abs(wide - wideBox)).toBeLessThan(2);
  expect(free).toBe("none");
  expect(capped).not.toBe("none");

  await tabs.nth(2).click();
  await expect(panel).toContainText("Coming…");
  await expect(panel).toContainText("how it moves");

  // The keys move the selection and the focus together, and wrap.
  await tabs.nth(0).click();
  await tabs.nth(0).focus();
  await page.keyboard.press("ArrowRight");
  await expect(tabs.nth(1)).toBeFocused();
  await expect(tabs.nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(tabs.nth(1)).toHaveAttribute("tabindex", "0");
  await expect(tabs.nth(0)).toHaveAttribute("tabindex", "-1");
  await page.keyboard.press("End");
  await expect(tabs.nth(2)).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect(tabs.nth(0)).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(tabs.nth(2)).toBeFocused();
  await page.keyboard.press("Home");
  await expect(tabs.nth(0)).toBeFocused();
  await expect(tabs.nth(0)).toHaveAttribute("aria-selected", "true");
  await expect(panel).toContainText("The overview text.");
  // The app's address is its route: a tab doesn't write it.
  expect(await page.evaluate(() => location.hash)).toBe("");
});

test("the strip is sticky at the top of the Doc and scrolls sideways when narrow", async ({ page }) => {
  await reset(page);
  const long = Array.from({ length: 12 }, (_, i) => ({ heading: `Section number ${i + 1}`, intent: "later" }));
  const d = await made(page, "Many tabs", "tabs", long);
  await fill(post, d, "s1", Array.from({ length: 80 }, (_, i) => `Line ${i + 1}.`).join("\n\n"));
  const block = page.locator(`[data-doc-block="${d}"]`);
  const strip = block.locator("[data-doc-tabs]");
  await expect(strip).toBeVisible({ timeout: 15_000 });
  // It doesn't wrap: it scrolls.
  expect(await strip.evaluate((e) => e.scrollWidth > e.clientWidth && getComputedStyle(e).overflowX === "auto")).toBe(true);
  expect(await strip.evaluate((e) => new Set([...e.children].map((c) => Math.round(c.getBoundingClientRect().top))).size)).toBe(1);
  // Scrolled down, the strip is still at the top of the Doc's scroll area.
  await block.evaluate((e) => (e.scrollTop = 600));
  const [top, docTop] = await Promise.all([strip.evaluate((e) => e.getBoundingClientRect().top), block.evaluate((e) => e.getBoundingClientRect().top)]);
  expect(Math.abs(top - docTop)).toBeLessThan(2);
  // The last tab, chosen by the keys, scrolls into view in the strip.
  await strip.getByRole("tab").first().focus();
  await page.keyboard.press("End");
  const last = strip.getByRole("tab").last();
  await expect(last).toHaveAttribute("aria-selected", "true");
  const inside = await last.evaluate((t) => {
    const bar = t.parentElement!.getBoundingClientRect();
    const r = t.getBoundingClientRect();
    return r.left >= bar.left - 1 && r.right <= bar.right + 1;
  });
  expect(inside).toBe(true);
});

test("a plain Doc has no tabs: its sections one after another, each with its heading", async ({ page }) => {
  await reset(page);
  const d = await made(page, "Plain one", "doc", TABS);
  await fill(post, d, "s1", "Plain text.");
  const block = page.locator(`[data-doc-block="${d}"]`);
  await expect(block.locator('[data-section="s1"]')).toContainText("Plain text.", { timeout: 15_000 });
  await expect(block.getByRole("tablist")).toHaveCount(0);
  await expect(block.locator(".doc-heading")).toHaveText(["Overview", "Corner tuner", "Motion"]);
});

test("a prototype Doc is its title, its one line and a frame that fills the view, and follows a resize", async ({ page }) => {
  await reset(page);
  // Tall enough that the frame is past its floor, so a resize shows.
  await page.setViewportSize({ width: 1400, height: 1000 });
  const d = await made(page, "Pomodoro", "prototype", [{ heading: "Pomodoro", intent: "a timer" }]);
  const block = page.locator(`[data-doc-block="${d}"]`);
  // Still to be written: a plain Doc, never a blank view.
  await expect(block.locator(".doc-coming")).toBeVisible({ timeout: 15_000 });
  await expect(block.locator("[data-doc-fill]")).toHaveCount(0);

  await fill(post, d, "s1", `A timer that counts down 25 minutes.\n\n${PROTO("Pomodoro")}`);
  await expect(block.locator("[data-doc-fill]")).toBeVisible({ timeout: 15_000 });
  await expect(block.locator(".doc-title")).toHaveText("Pomodoro");
  await expect(block.locator("[data-doc-line]")).toHaveText("A timer that counts down 25 minutes.");
  // No section heading, and the frame runs.
  await expect(block.locator(".doc-heading")).toHaveCount(0);
  await expect(block.getByRole("tablist")).toHaveCount(0);
  const frame = block.locator("[data-prototype-frame]");
  await expect(frame).toBeVisible();
  await expect(page.frameLocator(`[data-doc-block="${d}"] [data-prototype-frame]`).locator("#out")).toHaveText("ran", { timeout: 15_000 });

  // It fills what is left of the view under the title and its line: its bottom is the Doc's.
  const gap = () =>
    block.evaluate((b) => {
      const f = b.querySelector("[data-prototype-frame]")!.getBoundingClientRect();
      const doc = b.getBoundingClientRect();
      return { frame: f.height, bottom: Math.abs(doc.bottom - f.bottom) };
    });
  const before = await gap();
  expect(before.frame).toBeGreaterThan(300);
  expect(before.bottom).toBeLessThan(4);
  // Taller than the fence's height=300, edge to edge: the fence's number isn't what sizes it.
  const size = page.viewportSize()!;
  // Only the height changes, so the title and its line don't move: the frame follows the container.
  await page.setViewportSize({ width: size.width, height: size.height - 200 });
  // Both at once: the frame can shrink past it for a moment before the Doc settles (Arugula #955).
  await expect
    .poll(async () => {
      const g = await gap();
      return g.frame < before.frame - 100 && g.bottom < 4;
    })
    .toBe(true);
  await page.setViewportSize(size);
  await expect.poll(async () => (await gap()).frame).toBeGreaterThan(before.frame - 4);

  // The one line is cut to one line, and a click shows all of it.
  const line = block.locator("[data-doc-line]");
  await expect(line).toHaveAttribute("aria-expanded", "false");
  await line.click();
  await expect(line).toHaveAttribute("aria-expanded", "true");
});

test("the Docs page marks a tabs and a prototype Doc's row, and its preview draws each", async ({ page }) => {
  await reset(page);
  await tabsDoc(page, "Tabbed one");
  const proto = await made(page, "Prototype one", "prototype", [{ heading: "Prototype one", intent: "a thing" }]);
  await fill(post, proto, "s1", `One line about it.\n\n${PROTO("Thing")}`);
  const plain = await made(page, "Plain two", "doc", [{ heading: "Notes", intent: "text" }]);
  await fill(post, plain, "s1", "Some text.");
  await expect(page.locator(`[data-doc-block="${plain}"] .doc-title`)).toHaveText("Plain two", { timeout: 15_000 });

  await page.locator("[data-machine-docs]").click();
  await expect(page.locator("[data-docs-page]")).toBeVisible();
  const row = (t: string) => page.locator("[data-doc]").filter({ hasText: t });
  await expect(row("Tabbed one").getByRole("img", { name: "Tabs Doc" })).toHaveCount(1);
  await expect(row("Prototype one").getByRole("img", { name: "Prototype Doc" })).toHaveCount(1);
  await expect(row("Plain two").locator("[data-doc-layout]")).toHaveCount(0);

  // A tabs Doc's preview: its tabs, on the first, with ids that carry the Doc's own key.
  await row("Tabbed one").click();
  const pv = page.locator("[data-list-preview]");
  await expect(pv.getByRole("tab")).toHaveText(["Overview", "Corner tuner", "Motion"]);
  const lib = ((await (await fetch(`${url}/api/docs`)).json()) as { id: string; title: string }[]).find((r) => r.title === "Tabbed one")!.id;
  const first = pv.getByRole("tab").first();
  await expect(first).toHaveAttribute("aria-selected", "true");
  expect(await first.getAttribute("id")).toContain(lib);
  expect(await first.getAttribute("aria-controls")).toContain(lib);
  await expect(pv.getByRole("tabpanel")).toContainText("The overview text.");
  await pv.getByRole("tab").nth(1).click();
  await expect(pv.getByRole("tabpanel")).toContainText("The tuner text.");
  // The page's address is `#docs`: a tab neither leaves it nor adds to it.
  expect(await page.evaluate(() => location.hash)).toBe("#docs");
  await expect(page.locator("[data-docs-page]")).toBeVisible();

  // A prototype Doc's preview: the frame in the preview's own box, not a column.
  await row("Prototype one").click();
  await expect(pv.locator(".doc-title")).toHaveText("Prototype one");
  await expect(pv.locator("[data-doc-line]")).toHaveText("One line about it.");
  await expect(pv.locator("[data-prototype-frame]")).toBeVisible({ timeout: 15_000 });
  await expect(pv.locator(".doc-heading")).toHaveCount(0);
});
