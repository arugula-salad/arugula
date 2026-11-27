// Look (shared/kit/look.ts): the brand (design lift §4.4) paints Aaron's
// picks at Arugula's own sliders; every preset keeps its accent clear of
// the signals; the sliders move the neutrals and leave the signals; and
// whatever the preset, theme and sliders, every word reads at 4.5:1; and
// the terminals, editors and Docs are drawn from the same paint (#1511). Run
// with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ansi, contrast, house, mix, oklch, paint, paperUi, PRESETS, preset, sliders, tooClose, uiOf } from "../src/shared/kit/look.ts";

const themes = [["dark", true], ["light", false]];

test("Arugula at its own sliders is Aaron's picks: #78ce57 in dark, #3e5a05 in light", () => {
  assert.equal(PRESETS[0].id, "arugula");
  assert.equal(paint(true, sliders(true)).accent, "#78ce57");
  assert.equal(paint(false, sliders(false)).accent, "#3e5a05");
  // Light's fills stay bright, with dark words on them.
  const light = paint(false, sliders(false));
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
    for (const [name, dark] of themes) {
      assert.deepEqual(tooClose((dark ? p.dark : p.light).hue), [], `${p.id} ${name}`);
      const a = paint(dark, sliders(dark, p)).accent;
      assert.deepEqual(tooClose(oklch(a)[2]), [], `${p.id} ${name}: painted ${a}`);
    }
  assert.deepEqual(tooClose(60), ["needs"]);
  assert.deepEqual(tooClose(10), ["fail"]);
});

test("the sliders move the surfaces and leave the signals", () => {
  for (const [name, dark] of themes) {
    const at = (s) => paint(dark, { ...sliders(dark), ...s });
    assert.ok(oklch(at({ light: 1 }).canvas)[0] < oklch(at({ light: 0 }).canvas)[0], `${name}: darker`);
    const step = (p) => Math.abs(oklch(p.card)[0] - oklch(p.canvas)[0]);
    assert.ok(step(at({ depth: 0 })) < step(at({ depth: 1 })), `${name}: depth steps the surfaces`);
    assert.match(at({ depth: 0 })["shadow-card"], /rgb\(0 0 0 \/ 0\)/);
    // Warmth on an untinted preset (Arugula's dark tint is blue).
    const h = oklch(paint(dark, { ...sliders(dark), warmth: 1 }, preset("moss")).canvas)[2];
    assert.ok(h > 30 && h < 100, `${name}: warm canvas hue ${h}`);
    // Needs-you only steps its lightness to stay legible: its hue stays.
    const hue = (p) => oklch(p.needs)[2];
    assert.ok(Math.abs(hue(at({ warmth: 1, depth: 1 })) - hue(at({}))) < 4, `${name}: needs stays amber`);
  }
});

test("whatever the preset, theme and sliders, every word reads at 4.5:1 and needs-you stays amber", () => {
  const steps = [0, 0.25, 0.5, 0.75, 1];
  for (const p of PRESETS)
    for (const [name, dark] of themes)
      for (const light of steps)
        for (const warmth of steps)
          for (const depth of steps) {
            const t = paint(dark, { light, warmth, depth }, p);
            const at = `${p.id} ${name} ${light}/${warmth}/${depth}`;
            for (const word of ["ink", "ink-dim", "accent", "needs", "fail", "done", "review"])
              for (const on of ["canvas", "section", "card", "card-raised"]) {
                const r = contrast(t[word], t[on]);
                assert.ok(r >= 4.5, `${at}: --${word} on --${on} is ${r.toFixed(2)}`);
              }
            assert.ok(contrast(t["on-accent"], t["accent-fill"]) >= 4.5, `${at}: on-accent`);
            // The accent's words on its own tints: selected, "on", under the pointer.
            for (const on of ["canvas", "section", "card"])
              for (const k of [0.14, 0.18, 0.26]) {
                const r = contrast(t.accent, mix(t[on], t["accent-fill"], k));
                assert.ok(r >= 4.5, `${at}: --accent on its ${k * 100}% tint over --${on} is ${r.toFixed(2)}`);
              }
            assert.ok(contrast(t["on-needs"], t.needs) >= 4.5, `${at}: on-needs`);
            assert.ok(contrast(t["well-ink"], t.well) >= 4.5 && contrast(t["well-dim"], t.well) >= 4.5, `${at}: the well`);
            const [, , h] = oklch(t.needs);
            assert.ok(h >= 40 && h <= 80, `${at}: needs hue ${h}`);
          }
});

test("a terminal and an editor wear the look: the well, its ink, and sixteen colors that read on it", () => {
  for (const p of PRESETS)
    for (const [name, dark] of themes)
      for (const light of [0, 1]) {
        const t = paint(dark, { ...sliders(dark, p), light }, p);
        const u = uiOf(t, dark);
        const at = `${p.id} ${name} ${light}`;
        assert.equal(u.bg, t.well, `${at}: a terminal sits on the well`);
        for (const k of ["fg", "dim", "accent", "danger", "ok", "warn", "info", "peach", "teal", "pink", "merged", "subtle"])
          for (const on of ["bg", "surface", "raised"]) assert.ok(contrast(u[k], u[on]) >= 4.5, `${at}: ${k} on ${on} is ${contrast(u[k], u[on]).toFixed(2)}`);
        const c = ansi(t, dark);
        assert.equal(c.length, 16);
        // Red to cyan, plain and bright, read on the well.
        for (const i of [1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14, 15]) assert.ok(contrast(c[i], t.well) >= 4.5, `${at}: ansi ${i} ${c[i]} is ${contrast(c[i], t.well).toFixed(2)}`);
      }
});

test("a Doc's house pair is Arugula at its own sliders, whatever is picked, and reads on its paper", () => {
  for (const [name, dark] of themes) {
    const h = house(dark);
    const t = paint(dark, sliders(dark));
    assert.equal(h.paper, t.paper, name);
    assert.equal(h.ink, t.ink, name);
    assert.equal(h.live, t.accent, name);
    for (const k of ["ink", "ink-soft", "ink-faint", "link", "leaf", "pepper", "sky", "plum", "sun", "stone"])
      for (const on of ["paper", "tint"]) assert.ok(contrast(h[k], h[on]) >= 4.5, `${name}: ${k} on ${on} is ${contrast(h[k], h[on]).toFixed(2)}`);
    assert.equal(h.drawing, house(false).paper, `${name}: a drawing sits on the light paper`);
    // The prototype kit is on the same paper.
    assert.equal(paperUi(dark).bg, h.paper, name);
  }
});
