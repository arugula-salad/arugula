// The prototype kit (#699, shared/ui/runner.html): its colors are the
// palettes' own, and it is opt-in. appearance.ts is read as text, not imported
// (it reaches for the browser), so the test compares against the file itself.
// Run with `node --test`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const read = (rel) => readFileSync(new URL(rel, import.meta.url), "utf8");
const runner = read("../src/shared/ui/runner.html");
const kit = runner.match(/<template id="arugula-kit"><style>([\s\S]*?)<\/style><\/template>/)?.[1];

/** A palette's `ui: {...}` colors, by key, from appearance.ts's source. */
function palette(id) {
  const src = read("../src/shared/appearance.ts");
  const at = src.indexOf(`id: "${id}"`);
  assert.ok(at >= 0, `appearance.ts has no palette ${id}`);
  const ui = src.slice(at).match(/ui: \{([\s\S]*?)\n    \}/)[1];
  return Object.fromEntries([...ui.matchAll(/"?([\w-]+)"?: "(#[0-9a-fA-F]{6})"/g)].map((m) => [m[1], m[2]]));
}

// Each kit color token and the palette key it takes its value from.
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
  danger: "danger",
  ok: "ok",
  warn: "warn",
};

test("each kit color is Latte's in light and Slate's in dark, as appearance.ts has them", () => {
  assert.ok(kit, "runner.html has the kit's template");
  const latte = palette("latte");
  const slate = palette("slate");
  for (const [name, key] of Object.entries(TOKENS)) {
    const m = kit.match(new RegExp(`--ar-${name}: light-dark\\((#[0-9a-f]{6}), (#[0-9a-f]{6})\\);`));
    assert.ok(m, `--ar-${name} is declared as light-dark(Latte, Slate)`);
    assert.equal(m[1], latte[key], `--ar-${name} light is Latte's ${key}`);
    assert.equal(m[2], slate[key], `--ar-${name} dark is Slate's ${key}`);
  }
  // A new color token has to be named above, so it can't be copied by hand.
  const declared = [...kit.matchAll(/--ar-([\w-]+): light-dark\(#/g)].map((m) => m[1]);
  assert.deepEqual(declared.sort(), Object.keys(TOKENS).sort());
});

test("the kit is opt-in: its rules are :root's tokens and ar- classes, no element selectors", () => {
  const rules = [...kit.matchAll(/(?:^|\})\s*([^{}]+)\{/g)].map((m) => m[1].trim());
  assert.ok(rules.length > 20);
  for (const sel of rules) {
    for (const part of sel.replace(/:is\([^)]*\)/g, "").split(",").map((s) => s.trim())) {
      // `.ar-x`, or `.ar-x > :is(h1, h2)` style descendants of one; never a bare element.
      assert.ok(part === ":root" || part.startsWith(".ar-"), `${part} must start at an ar- class`);
    }
  }
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
