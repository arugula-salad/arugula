// One menu everywhere (#757), on a fixture page with no daemon
// (menus-page.tsx). A menu is a dropdown on a desktop and a sheet along the
// bottom at 390px, over the one backdrop, and a tap on the backdrop closes it
// and goes no further. A facet's ticks stay open. A menu opened from inside
// the board's side sheet, itself a bottom sheet at 390px, draws above it, and
// closes alone: by its backdrop and by Escape.
import { fileURLToPath } from "node:url";
import { expect, test, type Page } from "@playwright/test";
import { build } from "vite";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const ENTRY = fileURLToPath(new URL("./menus-page.tsx", import.meta.url));

type Out = {
  type: string;
  fileName: string;
  code?: string;
  source?: string | Uint8Array;
};
let html: Promise<string> | null = null;
/** The fixture page's HTML, script and style inline, built once per worker. */
function fixturePage(): Promise<string> {
  html ??= (async () => {
    const out = (await build({
      root: ROOT,
      configFile: false,
      oxc: { jsx: { runtime: "automatic", importSource: "preact" } },
      logLevel: "silent",
      build: {
        write: false,
        target: "es2022",
        lib: { entry: ENTRY, formats: ["es"], fileName: "page" },
        cssCodeSplit: false,
      },
    })) as { output: Out[] } | { output: Out[] }[];
    const files = Array.isArray(out)
      ? out.flatMap((o) => o.output)
      : out.output;
    const js = files.find((f) => f.type === "chunk")?.code ?? "";
    const css = files
      .filter((f) => f.fileName.endsWith(".css"))
      .map((f) => String(f.source))
      .join("\n");
    return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width"><style>${css}</style></head><body><div id="root"></div><script type="module">${js.replaceAll("</script", "<\\/script")}</script></body></html>`;
  })();
  return html;
}

async function open(page: Page, width: number, height = 800) {
  const body = await fixturePage();
  await page.setViewportSize({ width, height });
  await page.route("**/menus.html", (r) =>
    r.fulfill({ contentType: "text/html", body }),
  );
  await page.goto("/menus.html");
  await expect(page.locator("[data-open]")).toHaveCount(4);
}

const phone = {
  viewport: { width: 390, height: 800 },
  hasTouch: true,
} as const;

test("a desktop menu is a dropdown under its button, closed by Escape", async ({
  page,
}) => {
  await open(page, 1000);
  await page.locator('[data-open="menu"]').click();
  const menu = page.locator('[data-menu-name="Actions"]');
  await expect(menu).toBeVisible();
  await expect(menu).not.toHaveClass(/sheet/);
  await expect(page.locator("[data-menu-backdrop]")).toHaveCount(0);
  // Every row has the lead column, so the labels line up: a submenu's row
  // too, whose label once sat centred between its lead and its chevron.
  await expect(menu.locator(".menu-lead")).toHaveCount(4);
  const textLeft = (sel: string) =>
    menu.locator(sel).evaluate((el) => {
      const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, (n) => (n.textContent!.trim() ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP));
      const r = document.createRange();
      r.selectNodeContents(walk.nextNode()!);
      return r.getBoundingClientRect().left;
    });
  expect(Math.abs((await textLeft(".menu-parent")) - (await textLeft('[data-item="rename"]')))).toBeLessThan(1);
  // Two lines together become one, and none is left at the end.
  await expect(menu.locator(".menu-sep")).toHaveCount(1);
  expect(await menu.evaluate((m) => m.lastElementChild?.classList.contains("menu-sep"))).toBe(false);
  const button = (await page.locator('[data-open="menu"]').boundingBox())!;
  const box = (await menu.boundingBox())!;
  expect(box.y).toBeGreaterThanOrEqual(button.y + button.height);
  expect(box.width).toBeLessThan(400);
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  // The button is a toggle: a press on it while its menu is open shuts it.
  await page.locator('[data-open="menu"]').click();
  await expect(menu).toBeVisible();
  await page.locator('[data-open="menu"]').click();
  await expect(menu).toHaveCount(0);
});

test("a popover keeps the Tab key, gives the focus back on Escape, and is a toggle", async ({
  page,
}) => {
  await open(page, 1000);
  const button = page.locator('[data-open="popover"]');
  await button.click();
  const pop = page.locator("[data-popover]");
  await expect(pop).toBeVisible();
  await expect(pop).toHaveAttribute("role", "dialog");
  await expect(pop).toHaveAttribute("aria-label", "Details");
  await expect(button).toHaveAttribute("aria-expanded", "true");
  // Under its button, and the focus is inside it, on what asked for it.
  const at = (await button.boundingBox())!;
  expect((await pop.boundingBox())!.y).toBeGreaterThanOrEqual(at.y + at.height);
  await expect(page.locator('[data-pop="name"]')).toBeFocused();
  // Tab goes round inside it, both ways.
  await page.keyboard.press("Tab");
  await expect(page.locator('[data-pop="link"]')).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.locator('[data-pop="ok"]')).toBeFocused();
  await page.keyboard.press("Tab");
  await expect(page.locator('[data-pop="name"]')).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(page.locator('[data-pop="ok"]')).toBeFocused();
  // Escape shuts it and the focus is back on the button.
  await page.keyboard.press("Escape");
  await expect(pop).toHaveCount(0);
  await expect(button).toBeFocused();
  await expect(button).toHaveAttribute("aria-expanded", "false");
  // A press on the button while it is open shuts it and the click doesn't reopen it.
  await button.click();
  await expect(pop).toBeVisible();
  await button.click();
  await expect(pop).toHaveCount(0);
  // A click elsewhere shuts it.
  await button.click();
  await expect(pop).toBeVisible();
  await page.mouse.click(600, 500);
  await expect(pop).toHaveCount(0);
  // Another menu takes its place.
  await button.click();
  await page.locator('[data-open="menu"]').click();
  await expect(pop).toHaveCount(0);
  await expect(page.locator('[data-menu-name="Actions"]')).toBeVisible();
});

test.describe("on a phone", () => {
  test.use(phone);

  test("at 390px a popover is a sheet, and Escape gives the focus back", async ({
    page,
  }) => {
    await open(page, 390, 800);
    const button = page.locator('[data-open="popover"]');
    await button.click();
    const pop = page.locator("[data-popover]");
    await expect(pop).toHaveClass(/sheet/);
    const box = (await pop.boundingBox())!;
    expect(box.width).toBe(390);
    expect(Math.round(box.y + box.height)).toBe(800);
    await expect(page.locator("[data-menu-backdrop]")).toBeVisible();
    await expect(page.locator('[data-pop="name"]')).toBeFocused();
    await page.keyboard.press("Shift+Tab");
    await expect(page.locator('[data-pop="ok"]')).toBeFocused();
    await page.keyboard.press("Tab");
    await expect(page.locator('[data-pop="name"]')).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(pop).toHaveCount(0);
    await expect(button).toBeFocused();
    // A tap on the backdrop shuts it too.
    await button.click();
    await expect(pop).toBeVisible();
    await page.touchscreen.tap(200, 100);
    await expect(pop).toHaveCount(0);
  });

  test("at 390px a menu is a sheet along the bottom, and a tap on the backdrop closes it", async ({
    page,
  }) => {
    await open(page, 390, 800);
    await page.locator('[data-open="menu"]').click();
    const menu = page.locator('[data-menu-name="Actions"]');
    await expect(menu).toHaveClass(/sheet/);
    const box = (await menu.boundingBox())!;
    expect(box.x).toBe(0);
    expect(box.width).toBe(390);
    expect(Math.round(box.y + box.height)).toBe(800);
    // Rows are 44px under a finger, and a chord is not shown.
    expect(
      (await menu.locator('[data-item="copy"]').boundingBox())!.height,
    ).toBeGreaterThanOrEqual(44);
    await expect(menu.locator(".menu-key")).toBeHidden();
    // The backdrop is the one token.
    const backdrop = page.locator("[data-menu-backdrop]");
    await expect(backdrop).toBeVisible();
    const [bg, token] = await page.evaluate(() => [
      getComputedStyle(document.querySelector("[data-menu-backdrop]")!)
        .backgroundColor,
      (() => {
        const d = document.createElement("i");
        d.style.background = "var(--backdrop)";
        document.body.append(d);
        const c = getComputedStyle(d).backgroundColor;
        d.remove();
        return c;
      })(),
    ]);
    expect(bg).toBe(token);
    // A tap above the sheet closes it, and does not reach what is under (the
    // button at the bottom left is under the sheet, so tap where it is bare).
    await page.touchscreen.tap(200, 100);
    await expect(menu).toHaveCount(0);
    await expect(page.locator("[data-menu-backdrop]")).toHaveCount(0);
    await expect(page.locator("[data-ran]")).toHaveText("");
    // A row runs and closes it.
    await page.locator('[data-open="menu"]').click();
    await menu.locator('[data-item="rename"]').click();
    await expect(page.locator("[data-ran]")).toHaveText("rename");
    await expect(menu).toHaveCount(0);
  });
});

test("a facet's ticks stay open, and the count changes", async ({ page }) => {
  await open(page, 1000);
  const button = page.locator('[data-open="facet"]');
  await button.click();
  const menu = page.locator('[data-menu-name="Label"]');
  const docs = menu.locator('[data-facet-value="docs"]');
  await expect(docs).toHaveAttribute("aria-checked", "false");
  await docs.click();
  await expect(menu).toBeVisible();
  await expect(docs).toHaveAttribute("aria-checked", "true");
  await expect(docs.locator(".menu-lead svg")).toHaveCount(1);
  await expect(button).toContainText("· 1");
  await menu.locator('[data-facet-value="ui"]').check();
  await expect(button).toContainText("· 2");
  await docs.uncheck();
  await expect(button).toContainText("· 1");
  await expect(menu).toBeVisible();
});

test.describe("on a phone, in a side sheet", () => {
  test.use(phone);

  test("at 390px a menu opened from inside the side sheet draws above it and closes alone", async ({
    page,
  }) => {
    await open(page, 390, 800);
    await page.locator('[data-open="sheet"]').click();
    const sheet = page.locator("[data-sheet]");
    await expect(sheet).toBeVisible();
    await page.locator('[data-open="sheet-menu"]').click();
    const menu = page.locator('[data-menu-name="Row actions"]');
    await expect(menu).toHaveClass(/sheet/);
    // Two sheets, two backdrops: the menu's is the later and the higher.
    const z = (sel: string) =>
      page.evaluate(
        (s) => Number(getComputedStyle(document.querySelector(s)!).zIndex),
        sel,
      );
    expect(await z(".menu")).toBeGreaterThan(await z(".side-sheet-backdrop"));
    expect(await z("[data-menu-backdrop]")).toBeGreaterThan(
      await z(".side-sheet-backdrop"),
    );
    // The menu is what is on top at its own middle.
    const mb = (await menu.boundingBox())!;
    expect(
      await page.evaluate(
        ([x, y]) => !!document.elementFromPoint(x, y)?.closest(".menu"),
        [mb.x + mb.width / 2, mb.y + 20],
      ),
    ).toBe(true);
    // A tap on the menu's backdrop closes the menu only.
    await page.touchscreen.tap(200, 20);
    await expect(menu).toHaveCount(0);
    await expect(sheet).toBeVisible();
    // Escape: the menu first, the sheet second.
    await page.locator('[data-open="sheet-menu"]').click();
    await expect(menu).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);
    await expect(sheet).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(sheet).toHaveCount(0);
  });
});
