// The kit (design lift §4.1, Arugula #1478), on its living reference,
// /design: each of the fourteen parts renders, shows a focus ring, says its
// state in more than colour, and works by keyboard. `DESIGN_SHOTS=dir` saves
// the page in Slate and Latte, to look at.
import { expect, test, type Page } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const SHOTS = process.env.DESIGN_SHOTS;

async function open(page: Page) {
  const html = await fixturePage("./design-page.tsx");
  await page.route("**/design", (r) => r.fulfill({ contentType: "text/html", body: html }));
  await page.goto("http://design.test/design");
}

const section = (page: Page, id: string) => page.locator(`[data-design-section="${id}"]`);

test("every part renders in its section", async ({ page }) => {
  await open(page);
  const parts: [string, string][] = [
    ["surface", "surface"],
    ["row", "row"],
    ["need-row", "need-list"],
    ["well", "well"],
    ["state-mark", "state"],
    ["crumb", "crumb"],
    ["kbd", "kbd"],
    ["avatar", "avatar"],
    ["panel", "panel"],
    ["composer", "composer"],
    ["tabs", "tabs"],
    ["button", "button"],
    ["field", "field"],
  ];
  for (const [id, kit] of parts) {
    await expect(section(page, id).locator(`[data-kit="${kit}"]`).first(), id).toBeVisible();
    await expect(page.getByRole("navigation", { name: "Sections" }).locator(`a[href="#${id}"]`)).toHaveCount(1);
  }
  // Every role colour is a swatch.
  await expect(section(page, "tokens").locator('[data-design-swatch="--canvas"]')).toBeVisible();
  await expect(section(page, "tokens").locator('[data-design-swatch="--needs"]')).toBeVisible();
});

test("a part focused by keyboard shows the ring", async ({ page }) => {
  await open(page);
  await page.keyboard.press("Tab");
  const focusables = [
    section(page, "row").getByRole("button").first(),
    section(page, "crumb").getByRole("button").first(),
    section(page, "tabs").getByRole("tab", { selected: true }),
    section(page, "button").getByRole("button", { name: "Allow" }),
    section(page, "field").getByRole("switch"),
    section(page, "field").getByRole("radio", { checked: true }),
  ];
  for (const el of focusables) {
    await el.focus();
    const ring = await el.evaluate((e) => {
      const s = getComputedStyle(e);
      return { style: s.outlineStyle, width: parseFloat(s.outlineWidth) };
    });
    expect(ring.style).toBe("solid");
    expect(ring.width).toBeGreaterThanOrEqual(2);
  }
});

test("a state is never colour alone", async ({ page }) => {
  await open(page);
  for (const word of ["working", "asks", "waits", "stuck", "failing", "done", "idle"]) {
    await expect(section(page, "state-mark").locator(`[data-state="${word}"]`)).toContainText(word);
  }
  const needs = section(page, "need-row");
  await expect(needs.locator('[data-need="ask"]').first()).toContainText("asks you");
  await expect(needs.locator('[data-need="todo"]')).toContainText("waits on you");
  // A teammate's ask says whose, and has no amber tick.
  await expect(needs.getByText("asks Jake")).toBeVisible();
  await expect(section(page, "row").locator("[aria-current=true]")).toHaveCount(1);
  await expect(section(page, "field").getByRole("switch")).toHaveAttribute("aria-checked", "true");
});

test("NeedRow answers in place, by keyboard, at a third of the screen", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await open(page);
  const needs = section(page, "need-row");
  const ask = needs.locator('[data-need="ask"]').first();
  const line = ask.getByRole("button").first();
  await expect(line).toHaveAttribute("aria-expanded", "true");
  await line.focus();
  await page.keyboard.press("Enter");
  await expect(line).toHaveAttribute("aria-expanded", "false");
  await page.keyboard.press("Enter");
  await ask.getByRole("button", { name: "Truncate middle" }).focus();
  await page.keyboard.press("Enter");
  await expect(ask).toContainText("by you");
  await expect(ask.getByRole("button", { name: "Truncate middle", exact: true })).toHaveCount(0);

  // The to-do: its how in a well, a note marked no secrets, Done.
  const todo = needs.locator('[data-need="todo"]');
  await todo.getByRole("button").first().click();
  await expect(todo.getByRole("group", { name: "how" })).toContainText("just migrate");
  await todo.getByLabel("Note").fill("ran it");
  await expect(todo).toContainText("no secrets");
  await todo.getByLabel("Note").press("Enter");
  await expect(todo).toContainText("Done by you now · ran it");

  // 360px, a phone: nothing runs off the side.
  await page.setViewportSize({ width: 360, height: 800 });
  const perm = needs.locator('[data-need="permission"]');
  await perm.getByRole("button").first().click();
  for (const row of await needs.locator('[data-kit="row"]').all()) {
    expect(await row.evaluate((e) => e.scrollWidth <= e.clientWidth + 1)).toBe(true);
  }
  const allow = await perm.getByRole("button", { name: "Allow", exact: true }).boundingBox();
  expect(allow!.x + allow!.width).toBeLessThanOrEqual(360);
});

test("tabs, choice, composer and sheet work by keyboard", async ({ page }) => {
  await open(page);
  const tabs = section(page, "tabs");
  await tabs.getByRole("tab", { selected: true }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(tabs.getByRole("tab", { name: "#1443 worker" })).toHaveAttribute("aria-selected", "true");
  await expect(tabs.getByRole("tab", { name: "#1443 worker" })).toBeFocused();
  await page.keyboard.press("Delete");
  await expect(tabs.getByRole("tab", { name: "#1443 worker" })).toHaveCount(0);

  const range = section(page, "field").getByRole("radiogroup", { name: "Range" });
  await range.getByRole("radio", { checked: true }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(range.getByRole("radio", { name: "Month" })).toHaveAttribute("aria-checked", "true");
  await expect(range.getByRole("radio", { name: "Month" })).toBeFocused();

  const composer = section(page, "composer").getByRole("textbox");
  await composer.fill("hello");
  await composer.press("Enter");
  await expect(composer).toHaveValue("");

  const opener = section(page, "sheet").getByRole("button", { name: "Open a sheet" });
  await opener.focus();
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog", { name: "Remove this board?" });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("button", { name: "Remove" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(opener).toBeFocused();
});

test("Look repaints the page in either theme", async ({ page }) => {
  await open(page);
  await page.locator("body").click({ position: { x: 2, y: 2 } });
  await page.keyboard.press("s");
  const look = page.getByRole("dialog", { name: "Look" });
  await look.getByRole("radio", { name: "Latte" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-scheme", "light");
  const canvas = () => page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--canvas").trim());
  const before = await canvas();
  await look.getByRole("slider").first().fill("0.9");
  expect(await canvas()).not.toBe(before);
  await page.keyboard.press("Escape");

  if (SHOTS) {
    await page.setViewportSize({ width: 1440, height: 6000 });
    for (const scheme of ["Slate", "Latte"]) {
      await page.getByRole("button", { name: "Look" }).click();
      await page.getByRole("dialog", { name: "Look" }).getByRole("button", { name: "Reset" }).click();
      await page.getByRole("dialog", { name: "Look" }).getByRole("radio", { name: scheme }).click();
      await page.keyboard.press("Escape");
      await page.screenshot({ path: `${SHOTS}/design-${scheme.toLowerCase()}.png`, fullPage: true });
    }
  }
});
