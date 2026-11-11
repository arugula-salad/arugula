// The Docs page (#1494): the Docs a machine keeps, which outlive their pane
// (#1493). One daemon of its own. A Doc whose pane was closed is
// on the page and opens again in the current session with its content; one
// that is open comes forward instead of making a second; Delete asks, says
// when a link would stop working, and shows why a shared Doc stays when
// control can't be reached. The page fits a Pixel 7, and DOCS_PAGE_SHOTS=DIR
// keeps its four pictures (desktop and phone, light and dark). A shared
// Doc's menu after reopening, a delete that unshares, and Open from control's
// Shared Docs are in doc-share.spec.ts, which has a control to share with.
// The page is a list page (Arugula #587): a Doc's buttons are in the preview of
// the row that's selected, and the list's own keys and split are tested below.

import type { ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { fill, newDoc, png, putAsset, type Post } from "./doc-steps";
import { menu, open, panes, reset, sheetTab } from "./helpers";
import { daemon, pixel7 } from "./phones";

const MARK = "ZEBRA-QUARTZ-4471";
const SHOTS = process.env.DOCS_PAGE_SHOTS;

let dir = "";
let url = "";
let state = "";
let proc: ChildProcess | undefined;

test.describe.configure({ mode: "serial" });
test.use({ baseURL: async ({}, use) => use(url) });

test.beforeAll(async () => {
  dir = mkdtempSync(join(tmpdir(), "arugula-e2e-docs-page-"));
  state = join(dir, "state");
  ({ proc, url } = await daemon(state, ["--tailscale-socket", "/nonexistent/sock"]));
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

const row = (page: Page, title: string) => page.locator("[data-doc]").filter({ hasText: title });
/** The row selected, and its preview: where a Doc's buttons are. */
const preview = async (page: Page, title: string) => {
  await row(page, title).click();
  const p = page.locator("[data-list-preview]");
  // The Doc's own head is the preview's one title (Arugula #659).
  await expect(p.locator(".doc-title")).toHaveText(title);
  return p;
};
const askDelete = async (page: Page, title: string) => {
  const p = await preview(page, title);
  await p.locator("[data-doc-more]").click();
  await page.getByRole("menuitem", { name: "Delete…" }).click();
};
const goToDocs = async (page: Page) => {
  await page.locator("[data-machine-docs]").click();
  await expect(page.locator("[data-docs-page]")).toBeVisible();
};
const activePane = (page: Page) => page.evaluate(() => window.__arugula.client.active()!);
const typeOf = (page: Page, pane: number) => page.evaluate((p) => window.__arugula.client.info(p)?.type, pane);

/** A Doc beside the first pane with a word in its first section. */
async function doc(page: Page, title: string): Promise<number> {
  const [term] = await panes(page);
  const d = await newDoc(post, term, title);
  await fill(post, d, "s1", `Findings ${MARK}`);
  await expect(page.locator(`[data-doc-block="${d}"] .doc-title`)).toHaveText(title, { timeout: 15_000 });
  return d;
}

/** Link entries for Docs that were shared, as the machine keeps them: the
 * list reads them, and nothing is asked of control. */
function shared(docId: string, title: string) {
  const link = "0123456789abcdef0123456789abcdef";
  writeFileSync(
    join(state, "doc-links.json"),
    JSON.stringify({
      [link]: {
        ...{ key: "k".repeat(43), ver: 1, doc: docId, title },
        ...{ made: Date.now() - 86_400_000, updated: Date.now() - 3_600_000, size: 2048, viewer: "http://127.0.0.1:1/d" },
      },
    }),
  );
}

/** Where a Doc's copy on the platform is, as the machine notes it in the
 * Doc's `meta.json` after a put (#418): the list shows it (native #673). */
function onPlatform(docId: string) {
  const file = join(state, "docs", docId, "meta.json");
  const meta = JSON.parse(readFileSync(file, "utf8")) as Record<string, unknown>;
  const link = `http://127.0.0.1:1/p/prj_0123/docs/pdc_${"0".repeat(32)}`;
  meta.platform = { project: "prj_0123", id: `pdc_${"0".repeat(32)}`, link, rev: 3, assets: {} };
  writeFileSync(file, JSON.stringify(meta));
  return link;
}

const docId = async (title: string) => ((await (await fetch(`${url}/api/docs`)).json()) as { id: string; title: string }[]).find((r) => r.title === title)!.id;

test("with no Doc the page says where they come from", async ({ page }) => {
  await reset(page);
  await goToDocs(page);
  await expect(page.locator("[data-machine-docs]")).toHaveAttribute("aria-current", "page");
  await expect(page.locator("[data-docs-empty]")).toContainText("No Docs yet.");
  await expect(page.locator("[data-doc]")).toHaveCount(0);
});

test("a Doc whose pane was closed is on the page, and Open shows it again with its content", async ({ page }) => {
  await reset(page);
  const d = await doc(page, "Quarterly plan");
  await menu(page, page.locator(`[data-doc-block="${d}"]`), "Close pane");
  await expect(page.locator(`[data-doc-block="${d}"]`)).toHaveCount(0);

  await goToDocs(page);
  const r = row(page, "Quarterly plan");
  await expect(r).toHaveCount(1);
  await expect(r.locator("[data-doc-shared]")).toHaveCount(0);
  const p = await preview(page, "Quarterly plan");
  // The facts no longer say when it changed or its sections: the row has the age.
  await expect(p).not.toContainText(/updated (just now|\d+m ago)/);
  // Who made it is the facts line's hover now (Arugula #659), not a word on the bar.
  await expect(p.locator(".list-preview-bar-facts")).toHaveAttribute("title", /made by/);
  await p.locator("[data-open-doc]").click();

  // It left the page for a new block: the Doc, whole, and the one in front.
  await expect(page.locator("[data-docs-page]")).toHaveCount(0);
  const block = page.locator("[data-doc-block]");
  await expect(block).toHaveCount(1);
  await expect(block.locator(".doc-title")).toHaveText("Quarterly plan");
  await expect(block.locator('[data-section="s1"]')).toContainText(`Findings ${MARK}`);
  const now = await activePane(page);
  expect(now).not.toBe(d);
  expect(await typeOf(page, now)).toBe("doc");
  await expect(page.locator(`[data-doc-block="${now}"]`)).toBeVisible();
});

test("a Doc that is open comes forward, and a second block is not made", async ({ page }) => {
  await reset(page);
  const d = await doc(page, "Open one");
  const [term] = await panes(page);
  await page.evaluate((t) => window.__arugula.client.setActive(t), term);
  await goToDocs(page);
  const r = row(page, "Open one");
  await expect(r).toHaveAttribute("data-open", String(d));
  const p = await preview(page, "Open one");
  await expect(p.locator("[data-open-doc]")).toHaveText(`Bring forward %${d}`);
  await p.locator("[data-open-doc]").click();
  await expect(page.locator("[data-docs-page]")).toHaveCount(0);
  await expect.poll(() => activePane(page)).toBe(d);
  await expect(page.locator("[data-doc-block]")).toHaveCount(1);
});

test("Open puts it in the session you are in, not the first", async ({ page }) => {
  await reset(page);
  const d = await doc(page, "Second session");
  await post(`/api/panes/${d}/close`, {});
  await expect(page.locator(`[data-doc-block="${d}"]`)).toHaveCount(0);
  await page.evaluate(() => window.__arugula.client.intent({ op: "new_session", name: "other", from_pane: null }));
  await expect.poll(() => page.evaluate(() => window.__arugula.client.state!.sessions.length)).toBe(2);
  const here = await page.evaluate(() => window.__arugula.client.session);
  const first = await page.evaluate(() => window.__arugula.client.state!.sessions[0].id);
  expect(here).not.toBe(first);

  await goToDocs(page);
  await (await preview(page, "Second session")).locator("[data-open-doc]").click();
  await expect.poll(async () => typeOf(page, await activePane(page))).toBe("doc");
  const pane = await activePane(page);
  expect(await page.evaluate((p) => window.__arugula.client.sessionOfPane(p), pane)).toBe(here);
  await expect(page.locator(`[data-doc-block="${pane}"] .doc-title`)).toHaveText("Second session");
});

test("Delete asks first; a shared Doc's says its link stops working, and a refusal stays on the page", async ({ page }) => {
  await reset(page);
  await doc(page, "Plain one");
  await doc(page, "Shared one");
  await goToDocs(page);
  shared(await docId("Shared one"), "Shared one");
  await page.reload();
  await expect(page.locator("[data-docs-page]")).toBeVisible();

  // Not shared: it asks, and Cancel keeps it.
  await askDelete(page, "Plain one");
  const dialog = page.locator("[data-confirm-dialog]");
  await expect(dialog).toContainText("Delete “Plain one”?");
  await expect(dialog.locator("[data-delete-shared]")).toHaveCount(0);
  await page.locator("[data-cancel-remove]").click();
  await expect(row(page, "Plain one")).toHaveCount(1);

  // Shared: the link is said. This machine hasn't joined control, so it
  // can't unshare, and the Doc stays, with the reason on the page.
  const mine = row(page, "Shared one");
  await expect(mine.locator("[data-doc-shared]")).toBeVisible();
  await askDelete(page, "Shared one");
  await expect(dialog).toContainText("Delete “Shared one”?");
  await expect(dialog.locator("[data-delete-shared]")).toHaveText("Its link stops working.");
  await expect(page.locator("[data-confirm-remove]")).toBeEnabled();
  await page.locator("[data-confirm-remove]").click();
  await expect(page.locator("[data-docs-error]")).toContainText("can't unshare it at control, so the Doc is kept");
  await expect(mine).toHaveCount(1);

  // Plain: gone, and no longer on the page.
  await askDelete(page, "Plain one");
  await page.locator("[data-confirm-remove]").click();
  await expect(row(page, "Plain one")).toHaveCount(0);
  expect(((await (await fetch(`${url}/api/docs`)).json()) as { title: string }[]).map((r) => r.title)).not.toContain("Plain one");
});

/** The list page (Arugula #587, #589): what a row shows, its keys, its filters
 * and the split, on Docs the page has made of different lengths. */
test.describe("the list", () => {
  const long = Array.from({ length: 3000 }, (_, i) => `word${i}`).join(" ");
  const seed = async (page: Page) => {
    await reset(page);
    // Only these two: the Docs earlier tests left go first, none of them shared.
    writeFileSync(join(state, "doc-links.json"), "{}");
    for (const d of (await (await fetch(`${url}/api/docs`)).json()) as { id: string }[]) await fetch(`${url}/api/docs/${d.id}`, { method: "DELETE" });
    const [term] = await panes(page);
    const short = await newDoc(post, term, "Short note");
    await fill(post, short, "s1", "The opening line of the note.");
    const big = await newDoc(post, term, "Long plan");
    await fill(post, big, "s1", `Opening of the plan.\n\n${long}\n\n\`\`\`svg A picture\n<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="leaf"/></svg>\n\`\`\``);
    await post(`/api/panes/${short}/close`, {});
    await goToDocs(page);
    await expect(page.locator("[data-doc]")).toHaveCount(2);
    return { short, big };
  };

  test("a row is one line with its shape, and a long Doc's is longer", async ({ page }) => {
    await seed(page);
    const big = row(page, "Long plan");
    // The opening isn't under the title; the row is one line, not the 52px it was with it.
    await expect(row(page, "Short note").locator(".list-row-detail")).toBeHidden();
    await expect(row(page, "Short note")).not.toContainText("The opening line of the note.");
    await expect(big).not.toContainText("Opening of the plan.");
    expect((await row(page, "Short note").boundingBox())!.height).toBeLessThan(40);
    const w = async (t: string) => (await row(page, t).locator("[data-doc-shape]").boundingBox())!.width;
    expect(await w("Long plan")).toBeGreaterThan(await w("Short note"));
    // Its section with a figure is drawn as one, and only that one.
    await expect(big.locator("[data-doc-shape] i.fig")).toHaveCount(1);
    await expect(row(page, "Short note").locator("[data-doc-shape] i.fig")).toHaveCount(0);
    // Reading a closed Doc's preview doesn't open it.
    await expect((await preview(page, "Short note")).locator(".list-preview-words")).toContainText("The opening line of the note.");
    await expect(row(page, "Short note")).toHaveAttribute("data-open", "");
    await expect(page.locator(".block-doc [data-doc-block]")).toHaveCount(1);
  });

  test("a Doc copied up to the platform says so, and its preview opens the copy", async ({ page }) => {
    await seed(page);
    const link = onPlatform(await docId("Short note"));
    await page.reload();
    await goToDocs(page);
    await expect(row(page, "Short note").locator("[data-doc-platform]")).toHaveText("on the platform");
    await expect(row(page, "Long plan").locator("[data-doc-platform]")).toHaveCount(0);
    const p = await preview(page, "Short note");
    await expect(p.locator("[data-doc-platform-link]")).toHaveAttribute("data-doc-platform-link", link);
    await expect((await preview(page, "Long plan")).locator("[data-doc-platform-link]")).toHaveCount(0);
  });

  test("Copy link in the preview copies at once: the copy on the platform, and is off with none (Arugula #708)", async ({ page }) => {
    await seed(page);
    const link = onPlatform(await docId("Short note"));
    await page.reload();
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"], { origin: url });
    await goToDocs(page);
    const made: string[] = [];
    page.on("request", (req) => /doc_share|doc_link/.test(req.url()) && made.push(req.url()));
    const on = (await preview(page, "Short note")).locator("[data-doc-copy-link]");
    await expect(on).toBeEnabled();
    await on.click();
    await expect(on).toHaveText("Copied");
    await expect(on).toHaveAttribute("title", /only you can open it until it's shared/);
    await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe(link);
    expect(made, "nothing is asked of control and no link is made").toEqual([]);
    const none = (await preview(page, "Long plan")).locator("[data-doc-copy-link]");
    await expect(none).toBeDisabled();
    await expect(none).toHaveAttribute("title", "Not on the platform yet: sign in to Arugula control to get a link");
  });

  test("the preview's three actions: Copy link, Share and Open in browser, between Open beside and ⋯ (Arugula #782)", async ({ page }) => {
    await seed(page);
    const link = onPlatform(await docId("Short note"));
    await page.reload();
    await goToDocs(page);
    const p = await preview(page, "Short note");
    const bar = p.locator(".list-preview-bar");
    await expect(bar.locator("button")).toHaveCount(5);
    const order = await bar.locator("button").evaluateAll((bs) => bs.map((b) => b.getAttribute("aria-label") ?? b.textContent));
    expect(order.map((s) => s?.replace(/\s+/g, " "))).toEqual(["Open beside", "Copy link", "Share with a link", "Open in browser", "More for Short note"]);

    // Open in browser opens what Copy link copies, in a new tab.
    await page.context().route(link, (route) => route.fulfill({ body: "the copy" }));
    const popup = page.waitForEvent("popup");
    await bar.locator("[data-doc-open-browser]").click();
    expect((await popup).url()).toBe(link);

    // With no link it's off and says why.
    const none = (await preview(page, "Long plan")).locator("[data-doc-open-browser]");
    await expect(none).toBeDisabled();
    await expect(none).toHaveAttribute("title", "Not on the platform yet: sign in to Arugula control to get a link");

    // Share goes by the Doc's id (Arugula #819): its dialog, and no pane opens.
    const panes = async () => ((await (await page.request.get("/api/panes")).json()) as unknown[]).length;
    const before = await panes();
    await (await preview(page, "Short note")).locator("button[aria-label='Share with a link']").click();
    await expect(page.locator("[data-doc-share-dialog]")).toBeVisible();
    await expect(page.locator("[data-docs-page]")).toBeVisible();
    expect(await panes()).toBe(before);
  });

  test("the preview has exactly one title, and a bar of the house's buttons over it (Arugula #659)", async ({ page }) => {
    await seed(page);
    await page.reload();
    await goToDocs(page);
    const p = await preview(page, "Short note");
    // The Doc's own 38px head is the only title: no second one above it, none hidden.
    await expect(p.locator(".doc-title")).toHaveCount(1);
    await expect(p.locator(".doc-title")).toBeVisible();
    await expect(p.locator(".list-preview-head")).toHaveCount(0);
    // A 34px column bar: the facts at the left, then the primary, link and ⋯, last.
    const bar = p.locator(".list-preview-bar");
    expect((await bar.boundingBox())!.height).toBe(34);
    await expect(bar.locator(".list-preview-bar-facts")).toHaveText(/^\d+ min read$/);
    const buttons = bar.locator("button");
    await expect(buttons.first()).toHaveAttribute("data-open-doc", /.*/);
    await expect(buttons.last()).toHaveAttribute("data-doc-more", /.*/);
    await expect(bar.locator("[data-doc-copy-link]")).toHaveClass(/bar-btn sq/);
    for (const b of await buttons.all()) expect((await b.boundingBox())!.height).toBe(24);
  });

  test("a machine on a daemon older than the page still lists its Docs", async ({ page }) => {
    // Control serves its newest page to every machine, and a daemon before
    // 0.66.12 sends a row with no summary or sections, and has no preview.
    await seed(page);
    await page.route("**/api/docs", async (route) => {
      const res = await route.fetch();
      const rows = (await res.json()) as Record<string, unknown>[];
      await route.fulfill({ response: res, json: rows.map(({ summary: _summary, sections: _sections, ...old }) => old) });
    });
    await page.route("**/api/docs/*/preview", (route) => route.fulfill({ status: 404 }));
    await page.reload();
    await goToDocs(page);
    await expect(page.locator("[data-doc]")).toHaveCount(2);
    // Search still finds it by title, and its preview still has its buttons.
    await page.keyboard.press("/");
    await page.keyboard.type("short");
    await expect(page.locator("[data-doc]")).toHaveCount(1);
    const p = await preview(page, "Short note");
    await p.locator("[data-open-doc]").click();
    await expect(page.locator(`[data-doc-block] .doc-title`).filter({ hasText: "Short note" })).toBeVisible();
  });

  test("sweeping the pointer down the list changes nothing and reads nothing; a click shows the Doc", async ({ page }) => {
    await seed(page);
    const head = page.locator("[data-list-preview] .doc-title");
    await expect(head).toHaveText("Long plan");
    await expect(page.locator("[data-list-preview] .list-preview-words")).toContainText("Opening of the plan.");
    const reads: string[] = [];
    page.on("request", (r) => {
      if (/\/api\/docs\/[^/]+\/(content|preview)/.test(r.url())) reads.push(r.url());
    });
    // Slowly, with longer on each row than the old hover took to preview it.
    for (const title of ["Short note", "Long plan", "Short note"]) {
      const b = (await row(page, title).boundingBox())!;
      await page.mouse.move(b.x + 20, b.y + b.height / 2, { steps: 6 });
      await page.waitForTimeout(400);
      await expect(row(page, title)).toBeVisible();
    }
    await expect(head).toHaveText("Long plan");
    await expect(page.locator('[data-doc][aria-selected="true"]')).toContainText("Long plan");
    expect(reads).toEqual([]);
    // A click shows it, and a second click on it asks for nothing more.
    await row(page, "Short note").click();
    await expect(head).toHaveText("Short note");
    await expect(page.locator("[data-list-preview] .list-preview-words")).toContainText("The opening line of the note.");
    expect(reads.length).toBeGreaterThan(0);
    const after = reads.length;
    await row(page, "Short note").click();
    await page.waitForTimeout(300);
    expect(reads.length).toBe(after);
    // The keys still move it, and a double click opens it.
    await page.keyboard.press("ArrowUp");
    await expect(head).toHaveText("Long plan");
    await row(page, "Short note").dblclick();
    await expect(page.locator("[data-docs-page]")).toHaveCount(0);
    await expect(page.locator("[data-doc-block]")).toHaveCount(2);
  });

  test("↑↓ and j/k move, Enter opens, / searches and Esc clears", async ({ page }) => {
    await seed(page);
    const selected = page.locator('[data-doc][aria-selected="true"]');
    await expect(selected).toContainText("Long plan");
    await page.keyboard.press("ArrowDown");
    await expect(selected).toContainText("Short note");
    await expect(page.locator("[data-list-preview] .doc-title")).toHaveText("Short note");
    await page.keyboard.press("k");
    await expect(selected).toContainText("Long plan");
    await page.keyboard.press("j");
    await page.keyboard.press("/");
    await expect(page.locator("[data-list-search]")).toBeFocused();
    await page.keyboard.type("long");
    await expect(page.locator("[data-doc]")).toHaveCount(1);
    await page.keyboard.press("Escape");
    await expect(page.locator("[data-list-search]")).toHaveValue("");
    await expect(page.locator("[data-doc]")).toHaveCount(2);
    await row(page, "Short note").click();
    await page.keyboard.press("Enter");
    await expect(page.locator("[data-docs-page]")).toHaveCount(0);
    await expect(page.locator("[data-doc-block]")).toHaveCount(2);
  });

  test("the keys leave a dialog alone", async ({ page }) => {
    await seed(page);
    await askDelete(page, "Short note");
    await expect(page.locator("[data-confirm-dialog]")).toBeVisible();
    await page.keyboard.press("j");
    await expect(page.locator('[data-doc][aria-selected="true"]')).toContainText("Short note");
    await page.locator("[data-cancel-remove]").click();
  });

  test("the counts filter the list, and say when nothing matches", async ({ page }) => {
    await seed(page);
    await page.locator('[data-list-count="open"]').click();
    await expect(page.locator("[data-doc]")).toHaveCount(1);
    await expect(row(page, "Long plan")).toHaveCount(1);
    await page.locator('[data-list-count="shared"]').click();
    await expect(page.locator("[data-doc]")).toHaveCount(0);
    await expect(page.locator("[data-docs-none]")).toHaveText("No Doc is shared.");
    await expect(page.locator("[data-list-preview]")).toHaveCount(0);
    await page.locator('[data-list-count="shared"]').click();
    await expect(page.locator("[data-doc]")).toHaveCount(2);
  });

  test("a search finds a heading", async ({ page }) => {
    await seed(page);
    await page.locator("[data-list-search]").fill("risks");
    await expect(page.locator("[data-doc]")).toHaveCount(2);
    // The opening isn't drawn on the row, but a search still finds it.
    await page.locator("[data-list-search]").fill("opening line of the note");
    await expect(page.locator("[data-doc]")).toHaveCount(1);
    await expect(row(page, "Short note")).toBeVisible();
    await page.locator("[data-list-search]").fill("nothing like this");
    await expect(page.locator("[data-docs-none]")).toContainText("No Doc's title");
  });

  // The trap: a list that keeps its width crushes the preview to a word a line.
  for (const [width, both] of [[1600, true], [966, true], [700, false]] as const) {
    test(`at ${width} px it fits`, async ({ page }) => {
      await seed(page);
      // The rail goes at a phone's width: resize once the page is up.
      await page.setViewportSize({ width, height: 800 });
      const p = page.locator("[data-list-preview]");
      expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      if (!both) {
        await expect(p).toBeHidden();
        await page.locator("[data-doc]").first().click();
        await expect(p).toBeVisible();
        await expect(page.locator("[data-list-rows]")).toBeHidden();
        return;
      }
      const words = (await p.locator(".list-preview-words").boundingBox())!;
      expect(words.width).toBeGreaterThan(300);
      // The outline needs room beside the Doc: under 720 px of preview it folds away.
      if ((await p.boundingBox())!.width <= 720) return expect(p.locator(".list-preview-aside")).toBeHidden();
      const outline = (await p.locator(".list-preview-aside").boundingBox())!;
      const apart = words.x + words.width <= outline.x || words.y + words.height <= outline.y;
      expect(apart, "the text and the outline don't overlap").toBe(true);
    });
  }

  test("the divider drags, stays where it's put, and resets", async ({ page }) => {
    await page.setViewportSize({ width: 1400, height: 800 });
    await seed(page);
    const pane = page.locator(".list-pane");
    const start = (await pane.boundingBox())!.width;
    const h = (await page.locator(".list-handle").boundingBox())!;
    await page.mouse.move(h.x + h.width / 2, 300);
    await page.mouse.down();
    await page.mouse.move(h.x - 100, 300, { steps: 4 });
    await page.mouse.up();
    expect((await pane.boundingBox())!.width).toBeLessThan(start - 50);
    const put = (await pane.boundingBox())!.width;
    await page.reload();
    await expect(page.locator("[data-docs-page]")).toBeVisible();
    await expect.poll(async () => (await pane.boundingBox())?.width).toBe(put);
    await page.locator(".list-handle").dblclick();
    await expect.poll(async () => (await pane.boundingBox())?.width).toBe(start);
  });
});

/** The reader (#608): picking a row shows the whole Doc in the preview side. */
test.describe("the reader", () => {
  const proto = "```prototype height=120 Tune it\n<!doctype html><html><body><p id=out>ran</p></body></html>\n```";
  const pad = Array.from({ length: 60 }, (_, i) => `Line ${i} of a long section.`).join("\n\n");
  /** A closed Doc of three sections: a picture, a prototype, and a long one. */
  const seed = async (page: Page) => {
    await reset(page);
    writeFileSync(join(state, "doc-links.json"), "{}");
    for (const d of (await (await fetch(`${url}/api/docs`)).json()) as { id: string }[]) await fetch(`${url}/api/docs/${d.id}`, { method: "DELETE" });
    const [term] = await panes(page);
    const d = await newDoc(post, term, "Read me");
    await fill(post, d, "s1", "First section only.");
    await putAsset(post, d, "dot.png", png(40, 30));
    await fill(post, d, "s2", `Second: ![A red dot](assets/dot.png)\n\n${proto}`);
    await fill(post, d, "s3", `Third ${MARK}\n\n${pad}\n\nThe very end.`);
    await post(`/api/panes/${d}/close`, {});
    await goToDocs(page);
    return d;
  };
  const reader = (page: Page) => page.locator("[data-list-preview] [data-doc-reader]");

  test("a section past the first, a picture and a prototype show, and the Doc stays closed", async ({ page }) => {
    await seed(page);
    const p = await preview(page, "Read me");
    const r = reader(page);
    await expect(r.locator('[data-section="s2"]')).toContainText("Second:");
    await expect(r.locator('[data-section="s3"]')).toContainText(`Third ${MARK}`);
    await expect(r.locator('[data-section="s2"] img.doc-image')).toHaveJSProperty("naturalWidth", 40);
    await expect(r.locator("[data-prototype-frame]")).toHaveAttribute("title", "Tune it");
    // The Doc's own head is the title, said once (Arugula #659).
    await expect(r.locator(".doc-title")).toBeVisible();
    await expect(p.locator("[data-open-doc]")).toHaveText("Open beside");
    // Nothing opened it: the list still has it closed, and no block shows it.
    await expect(row(page, "Read me")).toHaveAttribute("data-open", "");
    await expect(page.locator(".block-doc [data-doc-block]")).toHaveCount(0);
    const rows = (await (await fetch(`${url}/api/docs`)).json()) as { title: string; open: number | null }[];
    expect(rows.find((x) => x.title === "Read me")!.open).toBeNull();
  });

  test("the outline scrolls the reader to a section", async ({ page }) => {
    await page.setViewportSize({ width: 1400, height: 700 });
    await seed(page);
    await preview(page, "Read me");
    const body = page.locator("[data-list-preview] .list-preview-body");
    await expect(reader(page).locator('[data-section="s3"]')).toBeVisible();
    expect(await body.evaluate((b) => b.scrollTop)).toBe(0);
    await page.locator("[data-doc-outline-go]").nth(2).click();
    await expect.poll(() => body.evaluate((b) => b.scrollTop)).toBeGreaterThan(100);
    const top = await reader(page).locator('[data-section="s3"]').evaluate((e) => e.getBoundingClientRect().top);
    const edge = await body.evaluate((b) => b.getBoundingClientRect().top);
    expect(Math.abs(top - edge)).toBeLessThan(40);
  });

  test("a fill while it's open shows when the window is back, and the scroll stays", async ({ page }) => {
    await page.setViewportSize({ width: 1400, height: 700 });
    await seed(page);
    const opened = (await post("/api/docs/open", { id: await docId("Read me") })) as { pane?: number };
    expect(opened.pane, "opened").toBeGreaterThan(0);
    await page.reload();
    await expect(page.locator("[data-docs-page]")).toBeVisible();
    await preview(page, "Read me");
    const body = page.locator("[data-list-preview] .list-preview-body");
    await page.locator("[data-doc-outline-go]").nth(2).click();
    await expect.poll(() => body.evaluate((b) => b.scrollTop)).toBeGreaterThan(100);
    const at = await body.evaluate((b) => b.scrollTop);
    await fill(post, opened.pane!, "s1", "First section, rewritten by an agent.");
    await page.evaluate(() => window.dispatchEvent(new Event("focus")));
    await expect(reader(page).locator('[data-section="s1"]')).toContainText("rewritten by an agent");
    expect(Math.abs((await body.evaluate((b) => b.scrollTop)) - at)).toBeLessThan(2);
  });
});

/** Three Docs in a daemon of its own: one shared, one open, one closed. */
test.describe("the page drawn", () => {
  let dir2 = "";
  let url2 = "";
  let proc2: ChildProcess | undefined;
  let seeded: Promise<void> | undefined;
  test.use({ baseURL: async ({}, use) => use(url2) });
  test.beforeAll(async () => {
    dir2 = mkdtempSync(join(tmpdir(), "arugula-e2e-docs-shots-"));
    ({ proc: proc2, url: url2 } = await daemon(join(dir2, "state"), ["--tailscale-socket", "/nonexistent/sock"]));
  });
  test.afterAll(() => {
    proc2?.kill("SIGKILL");
    if (dir2) rmSync(dir2, { recursive: true, force: true });
  });

  /** Made once, through the daemon's API, whichever test comes first. */
  const seed = (): Promise<void> =>
    (seeded ??= (async () => {
      const call: Post = async (path, body) => (await fetch(url2 + path, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) })).json();
      const [{ id: term }] = (await (await fetch(`${url2}/api/panes`)).json()) as { id: number }[];
      const made: number[] = [];
      for (const title of ["Quarterly plan", "Weekly report", "Incident review: the March outage and what we changed"]) {
        const d = await newDoc(call, term, title);
        await fill(call, d, "s1", `Findings ${MARK}`);
        made.push(d);
      }
      // The first two are closed; the third stays open.
      for (const d of made.slice(0, 2)) await call(`/api/panes/${d}/close`, {});
      const list = (await (await fetch(`${url2}/api/docs`)).json()) as { id: string; title: string }[];
      writeFileSync(
        join(dir2, "state", "doc-links.json"),
        JSON.stringify({
          "0123456789abcdef0123456789abcdef": {
            ...{ key: "k".repeat(43), ver: 1, doc: list.find((r) => r.title === "Weekly report")!.id, title: "Weekly report" },
            ...{ made: Date.now() - 86_400_000, updated: Date.now() - 3_600_000, size: 2048, viewer: "http://127.0.0.1:1/d" },
          },
        }),
      );
    })());

  const desktop = { width: 1280, height: 800 };
  for (const scheme of ["light", "dark"] as const) {
    test.describe(scheme, () => {
      test.use({ colorScheme: scheme });

      test("on a desktop: three Docs, one shared and one open", async ({ page }) => {
        await seed();
        await page.setViewportSize(desktop);
        if (scheme === "light") await page.addInitScript(() => localStorage.setItem("arugula-appearance", JSON.stringify({ palette: "latte", finish: "flat", corners: "square" })));
        await open(page);
        await goToDocs(page);
        await expect(page.locator("[data-doc]")).toHaveCount(3);
        await expect(page.locator("[data-doc-shared]")).toHaveCount(1);
        await expect(page.locator("[data-doc][data-open]:not([data-open=''])")).toHaveCount(1);
        if (SHOTS) {
          mkdirSync(SHOTS, { recursive: true });
          await page.screenshot({ path: join(SHOTS, `docs-desktop-${scheme}.png`) });
        }
      });

      test.describe("on a Pixel 7", () => {
        test.use(pixel7);
        test("the same fits the phone, reached from the sheet", async ({ page }) => {
          await seed();
          if (scheme === "light") await page.addInitScript(() => localStorage.setItem("arugula-appearance", JSON.stringify({ palette: "latte", finish: "flat", corners: "square" })));
          await open(page);
          await page.locator(".sheet-button").tap();
          await sheetTab(page, "More");
          await page.locator("[data-open-docs]").tap();
          await expect(page.locator("[data-docs-page]")).toBeVisible();
          await expect(page.locator("[data-doc]")).toHaveCount(3);
          // Nothing is wider than the phone, and every button is on it.
          const width = await page.evaluate(() => document.documentElement.clientWidth);
          expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
          // The list isn't focused on a touch screen: no ring on its first row.
          await expect(page.locator("[data-list-rows]")).not.toBeFocused();
          // A tap opens the preview in the list's place, and back returns.
          await page.locator("[data-doc]").first().tap();
          await expect(page.locator("[data-list-back]")).toBeVisible();
          await expect(page.locator("[data-list-rows]")).toBeHidden();
          for (const b of await page.locator("[data-list-preview] .list-preview-actions button").all()) {
            const box = (await b.boundingBox())!;
            expect(box.x + box.width).toBeLessThanOrEqual(width);
          }
          expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
          await page.locator("[data-list-back]").tap();
          await expect(page.locator("[data-list-rows]")).toBeVisible();
          if (SHOTS) {
            mkdirSync(SHOTS, { recursive: true });
            await page.screenshot({ path: join(SHOTS, `docs-pixel7-${scheme}.png`) });
          }
        });
      });
    });
  }
});

/** A Doc's times and maker as its `meta.json` keeps them: the list reads them from disk. */
function setMeta(id: string, patch: Record<string, unknown>) {
  const file = join(state, "docs", id, "meta.json");
  writeFileSync(file, JSON.stringify({ ...(JSON.parse(readFileSync(file, "utf8")) as Record<string, unknown>), ...patch }));
}

test("the list sorts by updated or created, each row says which, and the browser remembers (Arugula #825)", async ({ page }) => {
  await reset(page);
  await doc(page, "Tally old made");
  await doc(page, "Tally new made");
  const [oldMade, newMade] = [await docId("Tally old made"), await docId("Tally new made")];
  const day = 86_400_000;
  // Old was made first but touched last: Updated puts it on top, Created last.
  setMeta(oldMade, { made: Date.now() - 6 * day, updated: Date.now() - 3_600_000 });
  setMeta(newMade, { made: Date.now() - 2 * day, updated: Date.now() - 5 * 3_600_000 });
  await goToDocs(page);
  await page.locator("[data-list-search]").fill("Tally");
  const titles = () => page.locator("[data-doc] [data-doc-title]").allTextContents();
  const sort = page.locator("[data-list-sort]");

  await expect(sort).toHaveValue("updated");
  await expect.poll(titles).toEqual(["Tally old made", "Tally new made"]);
  await expect(row(page, "Tally old made").locator(".list-row-age")).toHaveText("edited 1h ago");

  await sort.selectOption("created");
  await expect.poll(titles).toEqual(["Tally new made", "Tally old made"]);
  await expect(row(page, "Tally new made").locator(".list-row-age")).toHaveText("created 2d ago");
  expect(await page.evaluate(() => localStorage.getItem("docs-sort"))).toBe("created");

  await page.reload();
  await page.locator("[data-list-search]").fill("Tally");
  await expect(sort).toHaveValue("created");
  await expect.poll(titles).toEqual(["Tally new made", "Tally old made"]);
});

test("an open Doc says when it was created and last edited; one from before Docs were kept says only the edit (Arugula #825)", async ({ page }) => {
  await reset(page);
  const d = await doc(page, "Stamped");
  const id = await docId("Stamped");
  const day = 86_400_000;
  setMeta(id, { made: Date.now() - 9 * day, updated: Date.now() - 3 * 3_600_000 });
  // The block's state is read when it opens: close it and open it again.
  await post(`/api/panes/${d}/close`, {});
  await goToDocs(page);

  const p = await preview(page, "Stamped");
  await expect(p.locator("[data-doc-times]")).toHaveText(/^Created \w+ \d+ · edited 3h ago$/);
  await expect(p.locator("[data-doc-created] time")).toHaveAttribute("title", /\d{4}.*\d:\d\d/);
  await p.locator("[data-open-doc]").click();
  const block = page.locator("[data-doc-block]");
  await expect(block.locator("[data-doc-times]")).toHaveText(/^Created \w+ \d+ · edited 3h ago$/);

  // Its made time is the migration's, or a file's: not said.
  setMeta(id, { by: "migrated" });
  await post(`/api/panes/${await activePane(page)}/close`, {});
  await goToDocs(page);
  const m = await preview(page, "Stamped");
  await expect(m.locator("[data-doc-times]")).toHaveText("Edited 3h ago");
  await expect(m.locator("[data-doc-created]")).toHaveCount(0);
});
