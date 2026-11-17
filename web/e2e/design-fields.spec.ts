// The design reference's Fields section (Arugula #1103): every control of
// the form kit, in its states, drawn by the real pieces. `DESIGN_SHOTS=dir`
// saves the section in Slate and Latte, to look at.
import { expect, test } from "@playwright/test";
import { fixturePage } from "./fixture-page";

const SHOTS = process.env.DESIGN_SHOTS;

test("the Fields section draws each control in its states", async ({ page }) => {
  const html = await fixturePage("./design-page.tsx");
  await page.route("**/design", (r) => r.fulfill({ contentType: "text/html", body: html }));
  await page.goto("http://design.test/design");
  const fields = page.locator('[data-design-section="fields"]');
  await expect(fields.getByRole("heading", { name: "Fields" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Sections" }).getByRole("link", { name: "Fields" })).toHaveAttribute("href", "#fields");

  // Text, select and textarea: filled, invalid and disabled.
  await expect(fields.getByLabel("Name, filled")).toHaveValue("build");
  await expect(fields.getByLabel("Name, invalid")).toHaveAttribute("aria-invalid", "true");
  await expect(fields.getByLabel("Name, disabled")).toBeDisabled();
  await expect(fields.getByLabel("Machine, disabled")).toBeDisabled();
  await expect(fields.getByLabel("Note, invalid")).toHaveAttribute("aria-invalid", "true");

  // The search field answers `/` from outside a field.
  await page.locator("body").click({ position: { x: 2, y: 2 } });
  await page.keyboard.press("/");
  await expect(page.locator("input:focus")).toHaveCount(1);

  // Boxes: off, on and some of a list; the dot; the switch's four looks.
  await expect(fields.getByRole("checkbox", { name: "Closed" })).toBeChecked();
  expect(await fields.getByRole("checkbox", { name: "All" }).evaluate((el) => (el as HTMLInputElement).indeterminate)).toBe(true);
  await expect(fields.getByRole("radio", { name: "Only me" })).toBeChecked();
  // A radio whose group has none picked is :indeterminate, and still draws off.
  const dot = fields.getByRole("radio", { name: "Pick one" });
  await expect(dot).not.toBeChecked();
  expect(await dot.evaluate((el) => getComputedStyle(el, "::before").opacity)).toBe("0");
  await expect(fields.getByRole("switch", { name: "On", exact: true })).toBeChecked();
  await expect(fields.locator(".sw-switch.partial")).toHaveCount(1);
  await expect(fields.getByRole("switch", { name: "On, disabled" })).toBeDisabled();

  // Segments, weekday chips and a time range that runs overnight.
  await fields.getByRole("group", { name: "Range", exact: true }).getByRole("button", { name: "Month" }).click();
  await expect(fields.getByRole("group", { name: "Range", exact: true }).getByRole("button", { name: "Month" })).toHaveAttribute("aria-pressed", "true");
  const days = fields.getByRole("group", { name: "Days", exact: true });
  await days.getByRole("button", { name: "Saturday" }).click();
  await expect(days.getByRole("button", { name: "Saturday" })).toHaveAttribute("aria-pressed", "true");
  await expect(fields.getByRole("group", { name: "Days, disabled" }).getByRole("button", { name: "Monday" })).toBeDisabled();
  await expect(fields.getByText("into the next morning")).toHaveCount(1);
  await expect(fields.getByLabel("Disabled from")).toBeDisabled();

  if (SHOTS) {
    // Tall enough that the page's own scroller holds the whole section.
    await page.setViewportSize({ width: 1100, height: 4000 });
    for (const scheme of ["slate", "latte"]) {
      await page.locator(`[data-design-scheme="${scheme}"]`).click();
      await page.getByLabel("Name, empty").focus();
      await fields.screenshot({ path: `${SHOTS}/fields-${scheme}.png` });
    }
  }
});
