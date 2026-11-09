// The design reference (#693), /design on the platform page: every shared
// piece of the UI drawn by the real components, for people and agents to
// check new UI against. This is its drift guard. What a piece list should
// hold is read from the source here (icons.tsx, the Tone type, style.css's
// tokens, appearance.ts's palette), not from the page, so a piece added or
// dropped without the page showing it fails a test by name.
//
// The page needs no one signed in, so it runs on a local control with the
// platform page (as boards-platform.spec.ts does) and no person. The last
// tests save a full-page screenshot in Slate and Latte, at 1280 and 390, to
// the run's output folder (test-results/): nothing is compared and nothing
// is committed; open them from the artifacts.

import { readFileSync } from "node:fs";
import { expect, test, type Browser, type Page } from "@playwright/test";
import { closeContexts } from "./helpers";
import { TeamControl } from "./team-fixture";

const t = new TeamControl("design");
let app = "";

test.beforeAll(async () => {
  test.setTimeout(60_000);
  await t.start(["--platform-host", "app.localhost"]);
  app = t.base.replace("//127.0.0.1", "//app.localhost");
});

test.afterAll(async ({ browser }) => {
  await closeContexts({ browser });
  t.stop();
});

const source = (path: string) => readFileSync(new URL(`../src/${path}`, import.meta.url), "utf8");

/** The page's sections: the language's, in its order (docs/design/language.md), with cards and icons, which it adds. */
const SECTIONS = ["type", "colour", "buttons", "headers", "rows", "chips", "cards", "icons", "dialogs", "menus", "states", "spacing"];

const WIDTHS = [
  { name: "desktop", width: 1280, height: 900 },
  { name: "phone", width: 390, height: 844 },
];

/** What the browser says is wrong: console errors and uncaught exceptions. */
function watch(page: Page): string[] {
  const bad: string[] = [];
  page.on("console", (m) => m.type() === "error" && bad.push(`console: ${m.text()}`));
  page.on("pageerror", (e) => bad.push(`pageerror: ${e.message}`));
  return bad;
}

async function open(browser: Browser, width = 1280, height = 900, hash = "") {
  const phone = width < 600;
  const context = await browser.newContext({ baseURL: app, viewport: { width, height }, ...(phone ? { isMobile: true, hasTouch: true } : {}) });
  const page = await context.newPage();
  const bad = watch(page);
  await page.goto(`/design${hash}`);
  await expect(page.locator("[data-design-page]")).toBeVisible({ timeout: 15_000 });
  await expect(page.locator("[data-design-section]")).toHaveCount(SECTIONS.length);
  return { page, bad };
}

test("every section of the language is on the page, with its anchor, its use and its import", async ({ browser }) => {
  const { page } = await open(browser);
  for (const id of SECTIONS) {
    const section = page.locator(`section#${id}[data-design-section="${id}"]`);
    await expect(section, id).toHaveCount(1);
    await expect(section.locator("h2").first(), `${id}: a heading`).not.toBeEmpty();
    await expect(section.locator("[data-design-use]"), `${id}: what it is for`).not.toBeEmpty();
    await expect(section.locator("[data-design-import]"), `${id}: where it is imported from`).toContainText("shared/");
    // More than its words: something is drawn.
    expect(await section.locator(".design-sample, .design-type, .design-swatches, .design-icons").count(), `${id}: samples`).toBeGreaterThan(0);
  }
  // The language's own sections are all here, by their names.
  const language = readFileSync(new URL("../../docs/design/language.md", import.meta.url), "utf8");
  const headings = [...language.matchAll(/^## (.+)$/gm)].map((m) => m[1]);
  for (const name of ["Type", "Colour", "Buttons", "Headers", "Rows and the selected state", "Chips", "Menus", "Empty and error states", "Spacing"]) {
    expect(headings, `language.md has ${name}`).toContain(name);
  }
});

test("an anchor in the address scrolls to its section", async ({ browser }) => {
  const { page } = await open(browser, 1280, 700, "#chips");
  await expect.poll(() => page.locator("#chips").evaluate((el) => el.getBoundingClientRect().top < innerHeight / 2 && el.getBoundingClientRect().bottom > 0)).toBe(true);
});

test("icons: every export of icons.tsx is drawn, by its name", async ({ browser }) => {
  const { page } = await open(browser);
  // The two wrappers draw an icon; every other export is one.
  const exported = [...source("shared/ui/icons.tsx").matchAll(/^export (?:const|function) ([A-Z]\w*)/gm)].map((m) => m[1]).filter((n) => n !== "Svg" && n !== "IconButton");
  expect(exported.length).toBeGreaterThan(40);
  const drawn = await page.locator("[data-design-icon]").evaluateAll((els) => els.map((el) => ({ name: el.getAttribute("data-design-icon"), svgs: el.querySelectorAll("svg").length, bad: el.querySelector(".design-icon-bad") !== null })));
  expect(drawn.map((d) => d.name).sort()).toEqual([...exported].sort());
  expect(drawn.filter((d) => d.svgs !== 1 || d.bad).map((d) => d.name), "each icon draws one svg").toEqual([]);
  expect(await page.locator("[data-design-icons]").getAttribute("data-design-icons")).toBe(String(exported.length));
  // One row at each size, and an IconButton with its name.
  for (const size of ["xs", "sm", "md", "lg"]) await expect(page.locator(`[data-design-icon-size="${size}"] svg.icon-${size}`)).toHaveCount(1);
  await expect(page.locator("[data-design-icon-button]")).toHaveAttribute("aria-label", "Search");
});

/** The members of `Tone`, from the file that defines it. */
const tones = () => [...(/export type Tone =([^;]+);/.exec(source("shared/ui/state-chip.tsx"))?.[1] ?? "").matchAll(/"([a-z]+)"/g)].map((m) => m[1]);

test("chips: all five shapes, and every tone of Tone", async ({ browser }) => {
  const { page } = await open(browser);
  const all = tones();
  expect(all.length).toBeGreaterThan(5);
  for (const tone of all) {
    await expect(page.locator(`.chip.chip-dot.chip-${tone}[data-design-chip="status-${tone}"]`), `status ${tone}`).toHaveCount(1);
    await expect(page.locator(`.chip.chip-tag.chip-${tone}[data-design-chip="tag-${tone}"]`), `tag ${tone}`).toHaveCount(1);
    await expect(page.locator(`.chip-object[data-design-chip="object-${tone}"] .chip-object-fact.chip-${tone}`), `object ${tone}`).toHaveCount(1);
  }
  await expect(page.locator('.count[data-design-chip="count"]')).toHaveCount(1);
  await expect(page.locator(".count.count-you")).toHaveCount(1);
  await expect(page.locator(".count.count-mention")).toHaveCount(1);
  await expect(page.locator(".chip-object.proof")).toHaveCount(1);
  await expect(page.locator(".chip-object.pair")).toHaveCount(1);
  await expect(page.locator("#chips .hint[data-hint]")).toHaveCount(1);
});

test("cards: every tone of Tone, plain, null, dense and with a bar", async ({ browser }) => {
  const { page } = await open(browser);
  for (const tone of tones()) await expect(page.locator(`.card.card-tone.card-${tone}[data-design-card="${tone}"]`), `card ${tone}`).toHaveCount(1);
  await expect(page.locator('.card[data-design-card="plain"]:not(.card-tone)')).toHaveCount(1);
  await expect(page.locator('.card.card-tone[data-design-card="null"]')).toHaveCount(1);
  await expect(page.locator('.card.card-dense[data-design-card="dense"]')).toHaveCount(1);
  await expect(page.locator('.card.has-bar[data-design-card="bar"] .hbar.sub')).toHaveCount(1);
});

test("the tokens on the page are the stylesheet's and the palette's", async ({ browser }) => {
  const { page } = await open(browser);
  const style = source("shared/style.css");
  // Type: every --text-* the stylesheet defines, each with a job.
  const steps = [...style.matchAll(/^\s+--text-([a-z0-9]+):/gm)].map((m) => m[1]);
  expect(steps.length).toBeGreaterThanOrEqual(7);
  expect(await page.locator("[data-design-type]").evaluateAll((els) => els.map((el) => el.getAttribute("data-design-type")))).toEqual(expect.arrayContaining(steps));
  await expect(page.locator("#type"), "every size has its job").not.toContainText("add its job");
  // Colour: a swatch for each of the palette's tokens.
  const ui = /interface Ui \{([\s\S]*?)\n\}/.exec(source("shared/appearance.ts"))![1];
  const colours = [...ui.matchAll(/^\s+"?([a-z-]+)"?: string;/gm)].map((m) => m[1]);
  expect(colours.length).toBeGreaterThan(12);
  expect((await page.locator("[data-design-swatch]").evaluateAll((els) => els.map((el) => el.getAttribute("data-design-swatch")))).sort()).toEqual([...colours].sort());
  // Spacing, corners, shadows and icon sizes.
  const names = (re: RegExp) => [...style.matchAll(re)].map((m) => m[1]);
  expect(await page.locator("[data-design-space]").evaluateAll((els) => els.map((el) => el.getAttribute("data-design-space")))).toEqual(expect.arrayContaining(names(/^\s+(--space-\d+):/gm)));
  expect(await page.locator("[data-design-radius]").evaluateAll((els) => els.map((el) => el.getAttribute("data-design-radius")))).toEqual(expect.arrayContaining(["--btn-radius", "--card-radius", "--panel-radius", "--tag-radius"]));
  expect(await page.locator("[data-design-shadow]").evaluateAll((els) => els.map((el) => el.getAttribute("data-design-shadow")))).toEqual(expect.arrayContaining(names(/^\s+(--shadow-(?:sm|md|lg)):/gm)));
  expect(await page.locator("[data-design-icon-token]").evaluateAll((els) => els.map((el) => el.getAttribute("data-design-icon-token")))).toEqual(expect.arrayContaining(names(/^\s+(--icon-[a-z]+):/gm)));
});

test("rows: a list page selects a row, and a card row has the selected state", async ({ browser }) => {
  const { page } = await open(browser);
  const rows = page.locator("[data-design-list] .list-row");
  await expect(rows).toHaveCount(3);
  await rows.nth(1).click();
  await expect(rows.nth(1)).toHaveAttribute("aria-selected", "true");
  await expect(page.locator('[data-design-card-row="b"]')).toHaveAttribute("aria-current", "true");
  await page.locator('[data-design-card-row="a"]').click();
  await expect(page.locator('[data-design-card-row="a"]')).toHaveAttribute("aria-current", "true");
  await expect(page.locator('[data-design-card-row="b"]')).not.toHaveAttribute("aria-current", "true");
});

test("a dialog, a confirm, a side sheet and a menu open, and Escape closes them", async ({ browser }) => {
  const { page, bad } = await open(browser);
  for (const [kind, dialog] of [
    ["modal", '[data-design-dialog="modal"]'],
    ["confirm", "[data-confirm-dialog]"],
    ["sheet", '[data-design-dialog="sheet"]'],
  ] as const) {
    const button = page.locator(`[data-design-open="${kind}"]`);
    await button.scrollIntoViewIfNeeded();
    await button.click();
    await expect(page.locator(dialog), kind).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator(dialog), `${kind} closes`).toHaveCount(0);
    await expect(button, `${kind} gives the focus back`).toBeFocused();
  }
  const button = page.locator('[data-design-open="menu"]');
  await button.scrollIntoViewIfNeeded();
  await button.click();
  const menu = page.locator('[role="menu"]');
  await expect(menu).toBeVisible();
  await expect(menu.locator(".menu-sep").first()).toBeAttached();
  await expect(menu.getByRole("menuitem", { name: "Close pane" })).toBeVisible();
  await expect(menu.getByRole("menuitemcheckbox", { name: "Open" })).toHaveAttribute("aria-checked", "true");
  await menu.getByRole("menuitem", { name: "Move to" }).click();
  await expect(menu.getByRole("menuitem", { name: "Next tab" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  // 13 rows or more: a filter row, which narrows the rows; Escape clears it, then closes.
  const long = page.locator('[data-design-open="long-menu"]');
  await long.scrollIntoViewIfNeeded();
  await long.click();
  const filter = menu.locator(".menu-filter");
  await expect(filter).toBeFocused();
  await page.keyboard.type("docs");
  await expect(menu.getByRole("menuitemcheckbox")).toHaveCount(2);
  await page.keyboard.press("Escape");
  await expect(filter).toHaveValue("");
  await expect(menu.getByRole("menuitemcheckbox")).toHaveCount(14);
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  expect(bad).toEqual([]);
});

test("the palette switch repaints the page and leaves the person's setting alone", async ({ browser }) => {
  const { page, bad } = await open(browser);
  const setting = () => page.evaluate(() => localStorage.getItem("arugula-appearance"));
  const before = await setting();
  const bg = () => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--bg").trim());
  const swatch = () => page.locator('[data-design-swatch="bg"] .design-swatch-value').textContent();
  await page.locator('[data-design-scheme="slate"]').click();
  const slate = await bg();
  await page.locator('[data-design-scheme="latte"]').click();
  await expect(page.locator("html")).toHaveAttribute("data-scheme", "light");
  expect(await bg()).not.toBe(slate);
  expect(await swatch(), "the swatch says the colour now painted").toBe(await bg());
  await page.locator('[data-design-scheme="slate"]').click();
  await expect(page.locator("html")).toHaveAttribute("data-scheme", "dark");
  expect(await bg()).toBe(slate);
  expect(await setting(), "the setting is unchanged").toBe(before);
  // A reload is back to the person's own.
  await page.locator('[data-design-scheme="latte"]').click();
  await page.reload();
  await expect(page.locator("html")).toHaveAttribute("data-scheme", "dark");
  expect(bad).toEqual([]);
});

test("no console errors on the page, in either palette, at any width", async ({ browser }) => {
  for (const w of WIDTHS) {
    const { page, bad } = await open(browser, w.width, w.height);
    for (const id of ["slate", "latte"]) {
      await page.locator(`[data-design-scheme="${id}"]`).click();
      for (const section of SECTIONS) await page.locator(`#${section}`).scrollIntoViewIfNeeded();
    }
    expect(bad, w.name).toEqual([]);
  }
});

test("the phone's page has sections stacked and nothing to scroll sideways", async ({ browser }) => {
  const { page } = await open(browser, 390, 844);
  const wide = await page.evaluate(() => {
    const over = [...document.querySelectorAll<HTMLElement>(".design-page *")].filter((el) => el.getBoundingClientRect().right > innerWidth + 1 && !el.closest(".list-pane, .side-sheet"));
    return { page: document.querySelector(".design-page")!.scrollWidth - document.querySelector(".design-page")!.clientWidth, root: document.documentElement.scrollWidth - innerWidth, over: over.slice(0, 5).map((el) => `${el.tagName}.${el.className}`) };
  });
  expect(wide).toEqual({ page: 0, root: 0, over: [] });
});

for (const w of WIDTHS) {
  for (const scheme of ["slate", "latte"]) {
    test(`screenshot: ${scheme} at ${w.width}`, async ({ browser }, info) => {
      test.setTimeout(60_000);
      const { page, bad } = await open(browser, w.width, w.height);
      await page.locator(`[data-design-scheme="${scheme}"]`).click();
      await expect(page.locator("html")).toHaveAttribute("data-scheme", scheme === "latte" ? "light" : "dark");
      await page.evaluate(() => document.fonts.ready);
      // The page scrolls inside the window: make the window as tall as the page.
      const height = await page.evaluate(() => document.querySelector(".design-page")!.scrollHeight);
      await page.setViewportSize({ width: w.width, height });
      await page.mouse.move(0, 0);
      // Let the cards' entrance finish.
      await page.waitForTimeout(500);
      const path = info.outputPath(`design-${scheme}-${w.width}.png`);
      await page.screenshot({ path, animations: "disabled" });
      await info.attach(`design-${scheme}-${w.width}`, { path, contentType: "image/png" });
      expect(bad).toEqual([]);
    });
  }
}
