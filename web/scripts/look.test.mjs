// Look (shared/kit/look.ts): the brand (design lift §4.4) paints Aaron's
// picks at Arugula's own sliders; every preset keeps its accent clear of
// the signals; the sliders move the neutrals and leave the signals; and
// whatever the preset, theme and sliders, every word reads at 4.5:1. Run
// with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { contrast, oklch, paint, PRESETS, preset, sliders, tooClose } from "../src/shared/kit/look.ts";

// appearance.ts's Slate and Latte: the legacy hues no role covers.
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
const themes = [["dark", slate, true], ["light", latte, false]];

test("Arugula at its own sliders is Aaron's picks: #78ce57 in dark, #3e5a05 in light", () => {
  assert.equal(PRESETS[0].id, "arugula");
  assert.equal(paint(slate, true, sliders(true)).accent, "#78ce57");
  assert.equal(paint(latte, false, sliders(false)).accent, "#3e5a05");
  // Light's fills stay bright, with dark words on them.
  const light = paint(latte, false, sliders(false));
  assert.ok(oklch(light["accent-fill"])[0] > 0.8, light["accent-fill"]);
  assert.ok(contrast(light["on-accent"], light["accent-fill"]) >= 4.5);
  assert.deepEqual(sliders(true), { light: 1, warmth: 0, depth: 0 });
  assert.equal(preset("nope").id, "arugula");
});

test("five presets, each accent kept 28° from amber and red, 22° from in-review blue", () => {
  assert.deepEqual(
    PRESETS.map((p) => p.name),
    ["Arugula", "Moss", "Sea glass", "Stone & ink", "Graphite & acid"],
  );
  for (const p of PRESETS)
    for (const [name, ui, dark] of themes) {
      assert.deepEqual(tooClose((dark ? p.dark : p.light).hue), [], `${p.id} ${name}`);
      const a = paint(ui, dark, sliders(dark, p)).accent;
      assert.deepEqual(tooClose(oklch(a)[2]), [], `${p.id} ${name}: painted ${a}`);
    }
  assert.deepEqual(tooClose(60), ["needs"]);
  assert.deepEqual(tooClose(10), ["fail"]);
});

test("the sliders move the surfaces and leave the signals", () => {
  for (const [name, ui, dark] of themes) {
    const at = (s) => paint(ui, dark, { ...sliders(dark), ...s });
    assert.ok(oklch(at({ light: 1 }).canvas)[0] < oklch(at({ light: 0 }).canvas)[0], `${name}: darker`);
    const step = (p) => Math.abs(oklch(p.card)[0] - oklch(p.canvas)[0]);
    assert.ok(step(at({ depth: 0 })) < step(at({ depth: 1 })), `${name}: depth steps the surfaces`);
    assert.match(at({ depth: 0 })["shadow-card"], /rgb\(0 0 0 \/ 0\)/);
    // Warmth on an untinted preset (Arugula's dark tint is blue).
    const h = oklch(paint(ui, dark, { ...sliders(dark), warmth: 1 }, preset("moss")).canvas)[2];
    assert.ok(h > 30 && h < 100, `${name}: warm canvas hue ${h}`);
    // Needs-you only steps its lightness to stay legible: its hue stays.
    const hue = (p) => oklch(p.needs)[2];
    assert.ok(Math.abs(hue(at({ warmth: 1, depth: 1 })) - hue(at({}))) < 4, `${name}: needs stays amber`);
  }
});

test("whatever the preset, theme and sliders, every word reads at 4.5:1 and needs-you stays amber", () => {
  const steps = [0, 0.25, 0.5, 0.75, 1];
  for (const p of PRESETS)
    for (const [name, ui, dark] of themes)
      for (const light of steps)
        for (const warmth of steps)
          for (const depth of steps) {
            const t = paint(ui, dark, { light, warmth, depth }, p);
            const at = `${p.id} ${name} ${light}/${warmth}/${depth}`;
            for (const word of ["ink", "ink-dim", "accent", "needs", "fail", "done", "review", "subtle", "merged"])
              for (const on of ["canvas", "section", "card", "card-raised"]) {
                const r = contrast(t[word], t[on]);
                assert.ok(r >= 4.5, `${at}: --${word} on --${on} is ${r.toFixed(2)}`);
              }
            assert.ok(contrast(t["on-accent"], t["accent-fill"]) >= 4.5, `${at}: on-accent`);
            assert.ok(contrast(t["on-needs"], t.needs) >= 4.5, `${at}: on-needs`);
            assert.ok(contrast(t["well-ink"], t.well) >= 4.5 && contrast(t["well-dim"], t.well) >= 4.5, `${at}: the well`);
            const [, , h] = oklch(t.needs);
            assert.ok(h >= 40 && h <= 80, `${at}: needs hue ${h}`);
          }
});
