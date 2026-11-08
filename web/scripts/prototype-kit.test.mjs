// The prototype kit (#699, shared/ui/runner.html): its colors are the ones the
// page paints, `legible()` of Latte and of Slate (appearance.ts), and it is
// opt-in. appearance.ts is imported as the page runs it: its imports leave off
// `.ts`, so a hook adds it. Run with `node --test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { registerHooks } from "node:module";
import { fileURLToPath } from "node:url";

registerHooks({
  resolve(spec, ctx, next) {
    if (spec.startsWith(".") && !/\.\w+$/.test(spec) && ctx.parentURL) {
      const url = new URL(`${spec}.ts`, ctx.parentURL);
      if (existsSync(fileURLToPath(url))) return { url: url.href, shortCircuit: true };
    }
    return next(spec, ctx);
  },
});
const { PALETTES, legible, contrast } = await import("../src/shared/appearance.ts");

const read = (rel) => readFileSync(new URL(rel, import.meta.url), "utf8");
const runner = read("../src/shared/ui/runner.html");
const kit = runner.match(/<template id="arugula-kit"><style>([\s\S]*?)<\/style><\/template>/)?.[1];

/** What the page paints for a palette: `apply()` sets exactly `legible(p)`. */
const painted = (id) => legible(PALETTES.find((p) => p.id === id));

// Each kit color token and the painted key it takes its value from.
const TOKENS = {
  bg: "bg",
  surface: "surface",
  raised: "raised",
  "raised-hover": "raised-hover",
  text: "fg",
  muted: "dim",
  subtle: "subtle",
  faint: "muted",
  border: "raised",
  accent: "accent",
  "on-accent": "on-color",
  "on-color": "on-color",
  danger: "danger",
  ok: "ok",
  warn: "warn",
  info: "info",
  merged: "merged",
  teal: "teal",
};

// The tokens that are not a pair but the app's own mix of the pair above.
const MIXED = {
  "line-strong": "color-mix(in srgb, var(--ar-faint) 55%, var(--ar-surface))",
  "on-fill": "color-mix(in srgb, var(--ar-accent) 18%, transparent)",
};

test("each kit color is what the page paints: Latte's legible() in light, Slate's in dark", () => {
  assert.ok(kit, "runner.html has the kit's template");
  const latte = painted("latte");
  const slate = painted("slate");
  for (const [name, key] of Object.entries(TOKENS)) {
    const m = kit.match(new RegExp(`--ar-${name}: light-dark\\((#[0-9a-f]{6}), (#[0-9a-f]{6})\\);`));
    assert.ok(m, `--ar-${name} is declared as light-dark(Latte, Slate)`);
    assert.equal(m[1], latte[key], `--ar-${name} light is Latte's painted ${key}`);
    assert.equal(m[2], slate[key], `--ar-${name} dark is Slate's painted ${key}`);
  }
  for (const [name, value] of Object.entries(MIXED)) {
    assert.equal(kit.match(new RegExp(`--ar-${name}: ([^;]*);`))?.[1], value, `--ar-${name} is the app's mix`);
  }
  // A new color token has to be named above, so it can't be copied by hand.
  const declared = [...kit.matchAll(/--ar-([\w-]+): light-dark\(#/g)].map((m) => m[1]);
  assert.deepEqual(declared.sort(), Object.keys(TOKENS).sort());
});

const channels = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const mix = (a, b, t) => `#${channels(a).map((v, i) => Math.round(v + (channels(b)[i] - v) * t).toString(16).padStart(2, "0")).join("")}`;

/** Each chip tone's text and edge tokens, from the kit's `.ar-chip-X` rules. */
function tones() {
  const out = {};
  for (const m of kit.matchAll(/\.ar-chip-(\w+) \{ --c: var\(--ar-([\w-]+)\); --ct: var\(--ar-([\w-]+)\); \}/g)) {
    out[m[1]] = { c: m[2], ct: m[3] };
  }
  return out;
}

test("each ar-chip tone reads at 4.5:1 on its own tint, in Latte and in Slate", () => {
  // The chip's fill is 12% of its color over the surface; the text must read on that.
  assert.match(kit, /\.ar-chip \{[^}]*background: color-mix\(in srgb, var\(--c\) 12%, var\(--ar-surface\)\);/);
  const t = tones();
  assert.deepEqual(Object.keys(t).sort(), ["accent", "danger", "info", "merged", "ok", "teal", "warn"]);
  // The base chip, with no tone: faint edge, subtle text.
  t.base = { c: "faint", ct: "subtle" };
  for (const id of ["latte", "slate"]) {
    const u = painted(id);
    const key = (name) => u[TOKENS[name]];
    for (const [tone, { c, ct }] of Object.entries(t)) {
      const tint = mix(u.surface, key(c), 0.12);
      const ratio = contrast(key(ct), tint);
      if (process.env.KIT_RATIOS) console.log(`| ${tone} | ${id} | ${key(ct)} | ${tint} | ${ratio.toFixed(2)} |`);
      assert.ok(ratio >= 4.5, `${id} ${tone}: ${key(ct)} on ${tint} is ${ratio.toFixed(2)}:1`);
    }
  }
});

test("the kit is opt-in: its rules are :root's tokens and ar- classes, no element selectors", () => {
  const rules = [...kit.replace(/\/\*[\s\S]*?\*\//g, "").matchAll(/(?:^|\})\s*([^{}]+)\{/g)].map((m) => m[1].trim());
  assert.ok(rules.length > 20);
  for (const sel of rules) {
    for (const part of sel.replace(/:is\([^)]*\)/g, "").split(",").map((s) => s.trim())) {
      // `.ar-x`, or `.ar-x > :is(h1, h2)` style descendants of one; never a bare element.
      assert.ok(part === ":root" || part.startsWith(".ar-"), `${part} must start at an ar- class`);
    }
  }
});

test("the kit has the chip and button shapes, each on --ar- tokens only", () => {
  for (const cls of ["ar-chip", "ar-chip-dot", "ar-tag", "ar-count", "ar-object", "ar-hint", "ar-bar-btn", "ar-seg"]) {
    assert.match(kit, new RegExp(`\\.${cls}[ ,:{]`), `${cls} is declared`);
  }
  assert.match(kit, /\.ar-seg > button:is\(\.on, /);
  assert.match(kit, /\.ar-bar-btn:is\(\.on, /);
  // No bare app token (`var(--accent)`) and no literal color outside the token block.
  const rules = kit.slice(Math.min(kit.indexOf(".ar-btn {"), kit.indexOf(".ar-card {")));
  assert.doesNotMatch(rules, /var\(--(?!ar-|c\b|ct\b)[\w-]+\)/);
  assert.doesNotMatch(rules, /#[0-9a-fA-F]{3,8}\b/);
});

test("the runner writes the kit into the prototype's document, after the policy", () => {
  assert.match(runner, /document\.getElementById\("arugula-kit"\)\.innerHTML/);
  const write = runner.slice(runner.indexOf("document.write("));
  assert.ok(write.indexOf("Content-Security-Policy") < write.indexOf("kit +"));
});

test("the kit's type scale is style.css's: the same sizes and line-heights", () => {
  const style = read("../src/shared/style.css");
  for (const step of ["xs", "sm", "md", "lg", "xl", "2xl", "doc"]) {
    const app = style.match(new RegExp(`--text-${step}: ([\\d.]+px);`))?.[1];
    assert.ok(app, `style.css declares --text-${step}`);
    assert.equal(kit.match(new RegExp(`--ar-text-${step}: ([\\d.]+px);`))?.[1], app, `--ar-text-${step} is ${app}`);
  }
  for (const step of ["xs", "sm", "md", "lg", "prose"]) {
    const app = style.match(new RegExp(`--lh-${step}: ([\\d.]+(?:px)?);`))?.[1];
    assert.ok(app, `style.css declares --lh-${step}`);
    assert.equal(kit.match(new RegExp(`--ar-lh-${step}: ([\\d.]+(?:px)?);`))?.[1], app, `--ar-lh-${step} is ${app}`);
  }
});

test("the kit's dialog sits under style.css's backdrop and has its title size", () => {
  const style = read("../src/shared/style.css");
  const app = style.match(/--backdrop: (rgb\([^)]*\));/)?.[1];
  assert.ok(app, "style.css declares --backdrop");
  assert.equal(kit.match(/--ar-backdrop: (rgb\([^)]*\));/)?.[1], app, `--ar-backdrop is ${app}`);
  const title = kit.match(/\.ar-dialog > :is\(h1, h2, h3\) \{([^}]*)\}/)?.[1] ?? "";
  assert.match(title, /font-size: var\(--ar-text-lg\);/);
  assert.match(title, /font-weight: 600;/);
  assert.match(kit, /\.ar-dialog > \.ar-dialog-buttons \{[^}]*justify-content: flex-end;/);
});

test("the kit's card is the app's: its sizes, its tones and Latte's tint", () => {
  const style = read("../src/shared/style.css");
  for (const [app, ar] of [["card-pad", "card-pad"], ["card-pad-dense", "card-pad-dense"], ["card-gap", "card-gap"], ["card-bar", "card-bar"]]) {
    const v = style.match(new RegExp(`--${app}: ([^;]+);`))?.[1];
    assert.ok(v, `style.css declares --${app}`);
    assert.equal(kit.match(new RegExp(`--ar-${ar}: ([^;]+);`))?.[1], v, `--ar-${ar} is ${v}`);
  }
  assert.equal(kit.match(/--ar-card-radius: ([^;]+);/)?.[1], "var(--ar-radius)");
  for (const cls of ["ar-card", "ar-card-tone", "ar-card-dense"]) assert.match(kit, new RegExp(`\\.${cls}[ ,:{]`), `${cls} is declared`);
  // The same seven tones as the app's card, each naming a kit color.
  const tones = [...kit.matchAll(/\.ar-card-(\w+) \{ --c: var\(--ar-([\w-]+)\); --ct: var\(--ar-([\w-]+)\); \}/g)].map((m) => m[1]);
  assert.deepEqual(tones.sort(), ["accent", "danger", "info", "muted", "ok", "teal", "warn"]);
  // The tint is style.css's: the oklch numbers of Latte, the mix of a dark palette.
  const all = (src, name) => [...src.matchAll(new RegExp(`--(?:ar-)?card-${name}: ([^;]+);`, "g"))].map((m) => m[1]).join(" | ");
  const oklch = (v) => v.match(/oklch\(from var\(--c\) .*? h\)/)?.[0];
  for (const [name, mixed] of [["edge", "color-mix(in srgb, var(--c) 45%, var(--ar-border))"], ["tint", "color-mix(in srgb, var(--c) 7%, var(--ar-surface))"]]) {
    const latte = oklch(all(style, name));
    assert.ok(latte, `style.css has Latte's ${name}`);
    assert.ok(all(kit, name).includes(latte), `the kit's --ar-card-${name} has Latte's ${latte}`);
    assert.ok(all(kit, name).includes(mixed), `and a dark palette's mix`);
  }
});

test("each ar-card tone reads at 4.5:1 on its dark tint (7% of the tone over the surface)", () => {
  const u = painted("slate");
  const key = (name) => u[TOKENS[name]];
  for (const m of kit.matchAll(/\.ar-card-(\w+) \{ --c: var\(--ar-([\w-]+)\); --ct: var\(--ar-([\w-]+)\); \}/g)) {
    const tint = mix(u.surface, key(m[2]), 0.07);
    const body = contrast(key("text"), tint);
    const title = contrast(key(m[3]), tint);
    assert.ok(body >= 4.5, `slate ${m[1]}: body text on ${tint} is ${body.toFixed(2)}:1`);
    assert.ok(title >= 4.5, `slate ${m[1]}: ${key(m[3])} on ${tint} is ${title.toFixed(2)}:1`);
  }
});
