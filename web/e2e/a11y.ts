// Accessibility checks (#401): axe on the main places, at a desktop width
// and a phone width, in the light and the dark theme, as Podium's suite
// does. a11y-shell.spec.ts scans a machine's own page (panes, swarm, Docs,
// settings), a11y-platform.spec.ts the platform page (projects, chat,
// boards, Docs, settings).
//
// The rules: WCAG 2.0, 2.1 and 2.2 at A and AA, which take in
// color-contrast, and target-size (2.5.8) turned on, since axe leaves it
// off by default. Not axe's best-practice rules (landmarks, regions):
// they're about how a page is laid out, not whether someone can use it.
//
// What a place already does wrong is in a11y-known.json, each with why. A
// scan fails on a violation that isn't listed, and on a listed one that
// didn't turn up, so the list shrinks as the fixes land: a fix has to take
// its line out. `sometimes: true` keeps a line out of that second check,
// for one that comes and goes with timing or turns up on one platform and
// not another (a size that follows the font: macOS and CI's Linux differ);
// its reason says which. Only those lines: the rest still fail when gone.
// A11Y_DUMP=DIR writes each place's violations there as JSON, to write the
// list from.

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { expect, type Page } from "@playwright/test";

export const WIDTHS = { desktop: { width: 1440, height: 900 }, phone: { width: 400, height: 860 } } as const;
export type Width = keyof typeof WIDTHS;
export const THEMES = ["light", "dark"] as const;
export type Theme = (typeof THEMES)[number];

const TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22a", "wcag22aa"];

/** A line of a11y-known.json. No `width` or `theme` means both. */
export interface Known {
  place: string;
  width?: Width;
  theme?: Theme;
  rule: string;
  target: string;
  reason: string;
  /** Comes and goes with timing, or with the platform's fonts: not
   * failed when it doesn't turn up. */
  sometimes?: boolean;
}

/** One element a rule flagged, where. */
export interface Found {
  place: string;
  width: Width;
  theme: Theme;
  rule: string;
  target: string;
  summary: string;
}

const known: Known[] = JSON.parse(readFileSync(new URL("./a11y-known.json", import.meta.url), "utf8"));

/** Follow the system's light or dark (the Appearance dialog's "System"),
 * which `colorScheme` then sets; reduced motion, so nothing is mid-fade when
 * axe reads its colors. Call before the page's first load: the swarm and
 * the terminals read reduced motion when they start. */
export async function prepare(page: Page) {
  await page.addInitScript(() => {
    try {
      const was = JSON.parse(localStorage.getItem("arugula-appearance") ?? "{}");
      localStorage.setItem("arugula-appearance", JSON.stringify({ ...was, palette: "system" }));
    } catch {
      // No storage: the default palette, dark.
    }
  });
  await page.emulateMedia({ reducedMotion: "reduce" });
}

/** The page at a width and a theme; `load` (re)opens the place after. */
export async function at(page: Page, width: Width, theme: Theme) {
  await page.setViewportSize(WIDTHS[width]);
  await page.emulateMedia({ colorScheme: theme, reducedMotion: "reduce" });
}

/** Wait for the page to settle: fonts in, no animation running, and the
 * DOM quiet for 300 ms (at most 5 s). */
export async function settle(page: Page) {
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  await page.evaluate(
    () =>
      new Promise<void>((done) => {
        const end = Date.now() + 5_000;
        let last = Date.now();
        const seen = new MutationObserver(() => (last = Date.now()));
        seen.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
        const tick = () => {
          const moving = document.getAnimations().some((a) => a.playState === "running" && a.effect?.getTiming().iterations !== Infinity);
          if ((!moving && Date.now() - last >= 300) || Date.now() > end) {
            seen.disconnect();
            done();
          } else setTimeout(tick, 50);
        };
        tick();
      }),
  );
}

/** A selector that says the same element from run to run: numbers that
 * are ids (a pane's, a room's) and long hex ids become `N` and `ID`. */
export const stable = (target: string) =>
  target
    .replace(/[0-9a-f]{12,}/g, "ID")
    .replace(/="\d+"/g, '="N"')
    .replace(/#([a-z-]*?)\d+\b/g, "#$1N");

/** axe on what the page shows now. */
export async function scan(page: Page, place: string, width: Width, theme: Theme): Promise<Found[]> {
  await settle(page);
  // options() replaces every option, so it goes before withTags().
  const { violations } = await new AxeBuilder({ page })
    .options({ rules: { "target-size": { enabled: true } } })
    .withTags(TAGS)
    .analyze();
  const out: Found[] = [];
  for (const v of violations)
    for (const n of v.nodes) {
      const target = stable(n.target.map((t) => (Array.isArray(t) ? t.join(" >> ") : t)).join(" >> "));
      const summary = (n.failureSummary ?? "").split("\n").slice(1, 2).join(" ").trim();
      if (!out.some((f) => f.rule === v.id && f.target === target)) out.push({ place, width, theme, rule: v.id, target, summary });
    }
  return out;
}

const covers = (k: Known, f: Found) =>
  k.place === f.place && k.rule === f.rule && k.target === f.target && (!k.width || k.width === f.width) && (!k.theme || k.theme === f.theme);

/** Check a place's scans (every width and theme it was scanned at) against
 * the known list: nothing new, and nothing listed that's gone. */
export function check(place: string, found: Found[]) {
  const dump = process.env.A11Y_DUMP;
  if (dump) {
    mkdirSync(dump, { recursive: true });
    writeFileSync(join(dump, `${place}.json`), JSON.stringify(found, null, 1));
  }
  const fresh = found.filter((f) => !known.some((k) => covers(k, f)));
  const gone = known.filter((k) => k.place === place && !k.sometimes && !found.some((f) => covers(k, f)));
  const line = (f: Found) => `  ${f.width} ${f.theme} ${f.rule} ${f.target}: ${f.summary}`;
  const said = (k: Known) => `  ${k.width ?? "any width"} ${k.theme ?? "any theme"} ${k.rule} ${k.target}`;
  expect(fresh.map(line), `${place}: violations not in a11y-known.json (fix them, or list them with why)`).toEqual([]);
  expect(gone.map(said), `${place}: listed in a11y-known.json but gone: take them out`).toEqual([]);
}

/** Scan `place` at every width and theme, `load` showing it each time. */
export async function everyWay(page: Page, place: string, load: (width: Width) => Promise<void>) {
  const found: Found[] = [];
  for (const width of Object.keys(WIDTHS) as Width[])
    for (const theme of THEMES) {
      await at(page, width, theme);
      // A load of its own each time: going to the same address with a
      // #fragment wouldn't reload.
      await page.goto("about:blank");
      await load(width);
      await expect(page.locator("html")).toHaveAttribute("data-scheme", theme);
      found.push(...(await scan(page, place, width, theme)));
    }
  check(place, found);
}
