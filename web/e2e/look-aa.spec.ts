// Look's sweep (design lift §4.3–4.4, Arugula #1477, #1482): Settings ›
// Appearance's five presets and three sliders, walked to the sliders' ends
// in both themes, as the page paints them. Every word reads at WCAG AA
// (4.5:1) on the page, a section, a card and a raised card; --needs (amber,
// asks you) never lands in the olive band; the sliders move the page; and
// Reset brings back Arugula's dark.
import { expect, test } from "@playwright/test";
import { closeLook, openLook, reset } from "./helpers";

const PRESETS = ["arugula", "moss", "sea-glass", "stone-ink", "graphite-acid"];

test("whatever the preset, theme and sliders, every word reads at 4.5:1 and needs-you stays amber", async ({ page }) => {
  await reset(page);
  const look = await openLook(page);
  for (const preset of PRESETS)
    for (const theme of ["dark", "light"]) {
      await look.locator(`[data-theme-choice="${theme}"]`).click();
      await look.locator(`[data-preset-choice="${preset}"]`).click();
      await expect(page.locator("html")).toHaveAttribute("data-scheme", theme);
      await expect(page.locator("html")).toHaveAttribute("data-preset", preset);
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
        const ends = [0, 100];
        for (const light of ends)
          for (const warmth of ends)
            for (const depth of ends) {
              slide("light", light);
              slide("warmth", warmth);
              slide("depth", depth);
              const s = css();
              const v = (k: string) => s.getPropertyValue(`--${k}`).trim();
              canvases.add(v("canvas"));
              for (const word of ["ink", "ink-dim", "accent", "needs", "fail", "done", "review"])
                for (const on of ["canvas", "section", "card", "card-raised"]) {
                  const r = ratio(v(word), v(on));
                  if (r < min) [min, at] = [r, `--${word} on --${on} at ${light}/${warmth}/${depth}`];
                }
              const h = hue(v("needs"));
              if (h >= 45 && h <= 75) olive.push(`${light}/${warmth}/${depth}: ${v("needs")}`);
            }
        return { min, at, olive, canvases: canvases.size };
      });
      expect(worst.min, `${preset} ${theme}: ${worst.at}`).toBeGreaterThanOrEqual(4.5);
      expect(worst.olive, `${preset} ${theme}`).toEqual([]);
      expect(worst.canvases, `${preset} ${theme}: light and warmth move the page`).toBe(4);
    }

  // Reset: Dark, Arugula, its sliders (darkest, cool, flat): Aaron's pick.
  await look.getByRole("button", { name: "Reset", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-scheme", "dark");
  await expect(page.locator("html")).toHaveAttribute("data-preset", "arugula");
  for (const [key, v] of [["light", "100"], ["warmth", "0"], ["depth", "0"]] as const) await expect(look.locator(`[data-look-slider="${key}"]`)).toHaveValue(v);
  const token = (k: string) => page.evaluate((k) => getComputedStyle(document.documentElement).getPropertyValue(`--${k}`).trim(), k);
  expect(await token("canvas")).toBe("#08121b");
  expect(await token("accent")).toBe("#78ce57");
  // Light starts at its own lightness, with Aaron's olive.
  await look.locator('[data-theme-choice="light"]').click();
  await expect(look.locator('[data-look-slider="light"]')).toHaveValue("78");
  expect(await token("accent")).toBe("#3e5a05");
  await closeLook(page);
});
