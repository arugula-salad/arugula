// Native #245: Search output from the command palette. A string printed in
// another session is found, and picking it shows that pane, scrolled to the
// line (it's selected).

import { expect, test } from "@playwright/test";
import { active, ready, reset, run } from "./helpers";

test.use({ viewport: { width: 1440, height: 900 } });

test("a line printed in another session is found, and picking it shows it there", async ({ page }) => {
  await reset(page);
  const home = await page.evaluate(() => window.__arugula.client.session!);
  await page.evaluate(() => window.__arugula.client.intent({ op: "new_session", name: "elsewhere", from_pane: null }));
  let other: number | undefined;
  await expect.poll(async () => (other = await page.evaluate(() => window.__arugula.client.state!.sessions.find((s) => s.name === "elsewhere")?.id))).not.toBeUndefined();
  await page.evaluate((s) => window.__arugula.client.selectSession(s), other!);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.session)).toBe(other);
  const there = await active(page);
  await ready(page, there);
  // The typed line has $((…)); only the output has the number.
  await run(page, there, "echo needle-$((6*7))-out", "needle-42-out");

  // On disk to search before the palette asks (it asks once per query).
  await expect
    .poll(async () => (await (await page.request.get("/api/search?re=needle-42-out")).json()).length)
    .toBeGreaterThan(0);

  await page.evaluate((s) => window.__arugula.client.selectSession(s), home);
  await expect.poll(() => page.evaluate(() => window.__arugula.client.session)).toBe(home);

  const mod = process.platform === "darwin" ? "Meta" : "Control";
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await page.keyboard.press(`${mod}+Shift+KeyP`);
  await palette.getByRole("combobox", { name: "Command" }).fill("Search output");
  await palette.locator('[data-command="Search/output"]').click();

  const search = palette.getByRole("combobox", { name: "Search output" });
  await expect(search).toBeFocused();
  await search.fill("needle-42-out");
  const hit = palette.locator(`[data-command^="Output/"][data-command*="/${there}/"]`, { hasText: "needle-42-out" });
  await expect(hit).toHaveCount(1, { timeout: 10_000 });
  // It says which machines it asked.
  await expect(palette.locator("[data-output-asked]")).toContainText("Searched");
  await expect(palette.getByRole("option").first()).toHaveAttribute("aria-selected", "true");
  const at = await palette.getByRole("option").evaluateAll((els) => els.findIndex((e) => e.textContent?.includes("needle-42-out")));
  for (let i = 0; i < at; i++) await search.press("ArrowDown");
  await search.press("Enter");
  await expect(palette).toBeHidden();

  await expect.poll(() => page.evaluate(() => window.__arugula.client.session)).toBe(other);
  await expect.poll(() => active(page)).toBe(there);
  await expect.poll(() => page.evaluate((p) => window.__arugula.selection(p), there)).toContain("needle-42-out");
});
