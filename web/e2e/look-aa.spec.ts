// Look's sweep (design lift §4.3, Arugula #1477): Settings › Appearance's
// three sliders, walked over a grid in both themes, as the page paints
// them. Every word reads at WCAG AA (4.5:1) on the page, a section and a
// raised card; --needs (amber, asks you) never lands in the olive band; the
// sliders move the page; and Reset brings back today's Slate.
import { expect, test } from "@playwright/test";
import { closeSettingsPage, openSettingsPage, reset } from "./helpers";

test("whatever the theme and sliders, every word reads at 4.5:1 and needs-you stays amber", async ({ page }) => {
  await reset(page);
  const look = (await openSettingsPage(page)).locator('[data-settings-section="appearance"]');
  for (const theme of ["dark", "light"]) {
    await look.locator(`[data-theme-choice="${theme}"]`).click();
    await expect(page.locator("html")).toHaveAttribute("data-scheme", theme);
    const worst = await page.evaluate(() => {
      const css = () => getComputedStyle(document.documentElement);
      const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
      const lum = (hex: string) => {
        const [r, g, b] = rgb(hex).map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
        return 0.2126 * r + 0.7152 * g + 0.0722 * b;
      };
      const ratio = (a: string, b: string) => {
        const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
        return (x + 0.05) / (y + 0.05);
      };
      const hue = (hex: string) => {
        const [r, g, b] = rgb(hex);
        const max = Math.max(r, g, b);
        const d = max - Math.min(r, g, b);
        if (!d) return -1;
        const h = max === r ? ((g - b) / d) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
        return (h * 60 + 360) % 360;
      };
      const slide = (key: string, v: number) => {
        const input = document.querySelector<HTMLInputElement>(`[data-look-slider="${key}"]`)!;
        input.value = String(v);
        input.dispatchEvent(new Event("input", { bubbles: true }));
      };
      let min = Infinity;
      let at = "";
      const olive: string[] = [];
      const canvases = new Set<string>();
      const steps = [0, 50, 100];
      for (const light of steps)
        for (const warmth of steps)
          for (const depth of steps) {
            slide("light", light);
            slide("warmth", warmth);
            slide("depth", depth);
            const s = css();
            const v = (k: string) => s.getPropertyValue(`--${k}`).trim();
            canvases.add(v("canvas"));
            for (const word of ["ink", "ink-dim", "accent", "needs", "fail", "done", "review"])
              for (const on of ["canvas", "section", "card-raised"]) {
                const r = ratio(v(word), v(on));
                if (r < min) [min, at] = [r, `--${word} on --${on} at ${light}/${warmth}/${depth}`];
              }
            const h = hue(v("needs"));
            if (h >= 45 && h <= 75) olive.push(`${light}/${warmth}/${depth}: ${v("needs")}`);
          }
      return { min, at, olive, canvases: canvases.size };
    });
    expect(worst.min, `${theme}: ${worst.at}`).toBeGreaterThanOrEqual(4.5);
    expect(worst.olive, theme).toEqual([]);
    expect(worst.canvases, `${theme}: light and warmth move the page`).toBe(9);
  }

  // Reset: Dark, each slider in the middle, today's Slate.
  await look.getByRole("button", { name: "Reset", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-palette", "slate");
  for (const key of ["light", "warmth", "depth"]) await expect(look.locator(`[data-look-slider="${key}"]`)).toHaveValue("50");
  expect(await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--canvas").trim())).toBe("#0c1118");
  await closeSettingsPage(page);
});
