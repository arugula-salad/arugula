// Look (shared/kit/look.ts): at the default sliders a theme paints exactly
// the colors it had; the sliders move the neutrals; whatever they say, every
// word reads at 4.5:1. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { contrast, fix, fromOklch, oklch, paint, slide, SLIDERS } from "../src/shared/kit/look.ts";

// appearance.ts's Slate and Latte.
const slate = {
  bg: "#0c1118", fg: "#d6dee9", dim: "#8b98ab", surface: "#090d13", letterbox: "#06090d", raised: "#1a2230", "raised-hover": "#243042",
  accent: "#56c8dc", danger: "#f2778a", ok: "#5fd394", warn: "#f0b45a", info: "#7aa7f0", peach: "#f29b6b", teal: "#5fd3c4", pink: "#ad9cf2",
  merged: "#b49cf2", muted: "#3a4658", subtle: "#b4c0d0", "on-color": "#06090d",
};
const latte = {
  bg: "#eff1f5", fg: "#4c4f69", dim: "#6c6f85", surface: "#e6e9ef", letterbox: "#dce0e8", raised: "#ccd0da", "raised-hover": "#bcc0cc",
  accent: "#8839ef", danger: "#d20f39", ok: "#40a02b", warn: "#df8e1d", info: "#1e66f5", peach: "#fe640b", teal: "#179299", pink: "#ea76cb",
  merged: "#8250df", muted: "#9ca0b0", subtle: "#5c5f77", "on-color": "#ffffff",
};
const themes = [["slate", slate, true], ["latte", latte, false]];

test("OKLCH round-trips a color", () => {
  for (const hex of ["#0c1118", "#eff1f5", "#56c8dc", "#df8e1d", "#000000", "#ffffff"]) assert.equal(fromOklch(oklch(hex)), hex);
});

test("at the default sliders a theme paints the colors it always had", () => {
  for (const [name, ui, dark] of themes) {
    assert.deepEqual(slide(ui, dark, SLIDERS), ui, name);
    const u = fix(ui, dark);
    const p = paint(ui, dark);
    const was = { canvas: u.bg, rail: u.surface, section: u.surface, card: u.surface, "card-raised": u.raised, well: u.letterbox, ink: u.fg, "ink-dim": u.dim, "ink-faint": u.muted, line: u.raised, hover: u["raised-hover"], accent: u.accent, needs: u.warn, fail: u.danger, review: u.info, done: u.ok, "on-accent": u["on-color"], subtle: u.subtle };
    for (const [k, v] of Object.entries(was)) assert.equal(p[k], v, `${name} --${k}`);
    assert.equal(p["shadow-k"], dark ? "1" : "0.35");
  }
  // Slate and Latte were legible already: fix() leaves them be.
  assert.deepEqual(fix(slate, true), slate);
});

test("the sliders move the surfaces and leave the signal colors", () => {
  for (const [name, ui, dark] of themes) {
    const darker = paint(ui, dark, { ...SLIDERS, light: 1 });
    assert.ok(oklch(darker.canvas)[0] < oklch(ui.bg)[0], `${name}: darker`);
    const flat = paint(ui, dark, { ...SLIDERS, depth: 0 });
    const deep = paint(ui, dark, { ...SLIDERS, depth: 1 });
    const step = (p) => Math.abs(oklch(p["card-raised"])[0] - oklch(p.canvas)[0]);
    assert.ok(step(flat) < step(deep), `${name}: depth steps the surfaces`);
    assert.equal(flat["shadow-k"], "0");
    const warm = paint(ui, dark, { ...SLIDERS, warmth: 1 });
    const h = oklch(warm.canvas)[2];
    assert.ok(h > 30 && h < 100, `${name}: warm canvas hue ${h}`);
    assert.equal(warm.needs, paint(ui, dark).needs, `${name}: needs stays`);
  }
});

test("the neutral tint never passes through green", () => {
  for (const [, ui, dark] of themes)
    for (let w = 0; w <= 1; w += 0.1) {
      const [, c, h] = oklch(paint(ui, dark, { ...SLIDERS, warmth: w }).canvas);
      assert.ok(c < 0.01 || h < 100 || h > 170, `hue ${h} at warmth ${w}`);
    }
});

test("every word reads at 4.5:1 on the page, a section and a raised card, whatever the sliders", () => {
  const steps = [0, 0.25, 0.5, 0.75, 1];
  for (const [name, ui, dark] of themes)
    for (const light of steps)
      for (const warmth of steps)
        for (const depth of steps) {
          const p = paint(ui, dark, { light, warmth, depth });
          for (const word of ["ink", "ink-dim", "accent", "needs", "fail", "done", "review"])
            for (const on of ["canvas", "section", "card-raised"]) {
              const r = contrast(p[word], p[on]);
              assert.ok(r >= 4.5, `${name} ${light}/${warmth}/${depth}: --${word} on --${on} is ${r.toFixed(2)}`);
            }
        }
});
