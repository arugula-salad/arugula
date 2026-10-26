// The Docs page (#1494): the Docs a machine keeps, which outlive their pane
// (#1493). One daemon with the `docs` flag. A Doc whose pane was closed is
// on the page and opens again in the current session with its content; one
// that is open comes forward instead of making a second; Delete asks, says
// when a link would stop working, and shows why a shared Doc stays when
// control can't be reached. The page fits a Pixel 7, and DOCS_PAGE_SHOTS=DIR
// keeps its four pictures (desktop and phone, light and dark). A shared
// Doc's menu after reopening, a delete that unshares, and Open from control's
// Shared Docs are in doc-share.spec.ts, which has a control to share with.

import type { ChildProcess } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { fill, newDoc, type Post } from "./doc-steps";
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
  ({ proc, url } = await daemon(state, ["--tailscale-socket", "/nonexistent/sock", "--wisp-token-file", "/nonexistent"]));
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
const goToDocs = async (page: Page) => {
  await page.locator(".ws-rail [data-open-docs]").click();
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

const docId = async (title: string) => ((await (await fetch(`${url}/api/docs`)).json()) as { id: string; title: string }[]).find((r) => r.title === title)!.id;

test("with no Doc the page says where they come from", async ({ page }) => {
  await reset(page);
  await goToDocs(page);
  await expect(page.locator(".ws-rail [data-open-docs]")).toHaveAttribute("aria-current", "page");
  await expect(page.locator("[data-docs-empty]")).toHaveText("Docs your agents write show up here.");
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
  await expect(r).toContainText(/written (just now|\d+m ago)/);
  await expect(r).toContainText("made by");
  await expect(r.locator("[data-doc-shared]")).toHaveCount(0);
  await r.locator("[data-open-doc]").click();

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
  await r.locator("[data-open-doc]").click();
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
  await row(page, "Second session").locator("[data-open-doc]").click();
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
  await row(page, "Plain one").locator("[data-delete-doc]").click();
  const dialog = page.locator("[data-confirm-dialog]");
  await expect(dialog).toContainText("Delete “Plain one”?");
  await expect(dialog.locator("[data-delete-shared]")).toHaveCount(0);
  await page.locator("[data-cancel-remove]").click();
  await expect(row(page, "Plain one")).toHaveCount(1);

  // Shared: the link is said. This machine hasn't joined control, so it
  // can't unshare, and the Doc stays, with the reason on the page.
  const mine = row(page, "Shared one");
  await expect(mine.locator("[data-doc-shared]")).toBeVisible();
  await mine.locator("[data-delete-doc]").click();
  await expect(dialog).toContainText("Delete “Shared one”?");
  await expect(dialog.locator("[data-delete-shared]")).toHaveText("Its link stops working.");
  await expect(page.locator("[data-confirm-remove]")).toBeEnabled();
  await page.locator("[data-confirm-remove]").click();
  await expect(page.locator("[data-docs-error]")).toContainText("can't unshare it at control, so the Doc is kept");
  await expect(mine).toHaveCount(1);

  // Plain: gone, and no longer on the page.
  await row(page, "Plain one").locator("[data-delete-doc]").click();
  await page.locator("[data-confirm-remove]").click();
  await expect(row(page, "Plain one")).toHaveCount(0);
  expect(((await (await fetch(`${url}/api/docs`)).json()) as { title: string }[]).map((r) => r.title)).not.toContain("Plain one");
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
    ({ proc: proc2, url: url2 } = await daemon(join(dir2, "state"), ["--tailscale-socket", "/nonexistent/sock", "--wisp-token-file", "/nonexistent"]));
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
          for (const b of await page.locator("[data-doc] button").all()) {
            const box = (await b.boundingBox())!;
            expect(box.x + box.width).toBeLessThanOrEqual(width);
          }
          if (SHOTS) {
            mkdirSync(SHOTS, { recursive: true });
            await page.screenshot({ path: join(SHOTS, `docs-pixel7-${scheme}.png`) });
          }
        });
      });
    });
  }
});
