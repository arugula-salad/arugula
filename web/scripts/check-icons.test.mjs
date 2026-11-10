import assert from "node:assert/strict";
import { test } from "node:test";
import { ALLOWED_SVG, normalise, offenders } from "./check-icons.mjs";

const bad = (src, file = "x.tsx") => offenders(src, file).map((o) => [o.line, o.rule]);

test("a glyph of the set as JSX text is refused, at either end of what's said", () => {
  assert.deepEqual(bad("<span>▾</span>"), [[1, "glyph"]]);
  assert.deepEqual(bad("<button>\n  Records ▾\n</button>"), [[2, "glyph"]]);
  assert.deepEqual(bad("<a>↗ Open</a>"), [[1, "glyph"]]);
  assert.deepEqual(bad("<a>{label} ↗</a>"), [[1, "glyph"]]);
  for (const g of "▾▸⋯✕×✓↗‹›↻✎⤢★☆↩↑↓→") assert.equal(offenders(`<b>${g}</b>`, "x.tsx").length, 1, g);
});

test("a glyph a JSX child chooses is refused too", () => {
  assert.deepEqual(bad('<b>{open ? "▾" : "▸"}</b>'), [[1, "glyph"], [1, "glyph"]]);
  assert.deepEqual(bad('<b>{on && "✓"}</b>'), [[1, "glyph"]]);
  assert.deepEqual(bad("<b>{busy ? '…' : '↻'}</b>"), [[1, "glyph"]]);
});

test("what stays text stays: …, ⌘, a sentence's glyph, key names, counts, a prompt, an arrow between values", () => {
  assert.deepEqual(bad("<b>Loading…</b>"), []);
  assert.deepEqual(bad("<b>⌘K</b>"), []);
  assert.deepEqual(bad("<b>(Share › Add to Home Screen) and open it</b>"), []);
  assert.deepEqual(bad("<b><kbd>↑</kbd> <kbd>↓</kbd> Navigate</b>"), []);
  assert.deepEqual(bad("<b>Claude ×2</b>"), []);
  assert.deepEqual(bad("<b>×{n}</b>"), []);
  assert.deepEqual(bad("<b>›_</b>"), []);
  assert.deepEqual(bad("<b>{from} → {to}</b>"), []);
  assert.deepEqual(bad('const keys = { label: "↑", bytes: "x" };'), []);
  assert.deepEqual(bad("// a ▾ in a comment\n/* and ✓ here */\n<b>{/* ▾ */}</b>"), []);
});

test("a line says why a glyph is text, on itself or the line above", () => {
  assert.deepEqual(bad("{/* lint-icons: text — a rename's arrow */}\n<span>{old} → </span>"), []);
  assert.deepEqual(bad("<span>{old} → </span> {/* lint-icons: text — a rename's arrow */}"), []);
  assert.deepEqual(bad("{/* lint-icons: text */}\n<span>{old} → </span>"), [[2, "glyph"]]);
});

test("a stylesheet's content: is refused a glyph, not a dot", () => {
  assert.deepEqual(bad('.a::before { content: "▾"; }', "x.css"), [[1, "content"]]);
  assert.deepEqual(bad('.a::after {\n  content: "✓ ";\n}', "x.css"), [[2, "content"]]);
  assert.deepEqual(bad('.a::before { content: "▾" / ""; }', "x.css"), [[1, "content"]]);
  assert.deepEqual(bad('.a::before { content: "● "; }', "x.css"), []);
  assert.deepEqual(bad('/* content: "▾" */ .a { color: red; }', "x.css"), []);
  assert.deepEqual(bad(".a::before { content: ''; }\n.b { font: 13px x; }", "x.css"), []);
});

test("a number as an icon's size is refused; a name or a variable is not", () => {
  assert.deepEqual(bad("<PlusIcon size={13} />"), [[1, "size"]]);
  assert.deepEqual(bad('<Svg size={14}>\n  <path d="" />\n</Svg>'), [[1, "size"]]);
  assert.deepEqual(bad('<MoreIcon size="sm" />'), []);
  assert.deepEqual(bad("<MoreIcon size={size} />"), []);
  assert.deepEqual(bad("<img size={13} />"), []);
});

test("an <svg> outside icons.tsx is refused, unless it is on the list", () => {
  assert.deepEqual(bad('<svg viewBox="0 0 24 24"></svg>', "shell/ui/x.tsx"), [[1, "svg"]]);
  assert.deepEqual(bad('<svg viewBox="0 0 24 24"></svg>', "shared/ui/icons.tsx"), []);
  for (const file of Object.keys(ALLOWED_SVG)) assert.deepEqual(bad("<svg></svg>", file), [], file);
  assert.deepEqual(bad("// <svg> is drawn in icons.tsx", "shell/ui/x.tsx"), []);
});

test("a path with Windows separators names the same file", () => {
  assert.equal(normalise("shared\\ui\\icons.tsx"), "shared/ui/icons.tsx");
  assert.deepEqual(bad("<svg></svg>", "shared\\ui\\icons.tsx"), []);
  assert.deepEqual(bad("<svg></svg>", "shell\\doc-view.tsx"), []);
  assert.deepEqual(bad("<svg></svg>", "shell\\ui\\x.tsx"), [[1, "svg"]]);
  assert.deepEqual(bad('.a::before { content: "▾"; }', "shared\\x.css"), [[1, "content"]]);
});

test("an icon-only button needs both a title and an aria-label", () => {
  assert.deepEqual(bad("<button onClick={go}>\n  <CloseIcon />\n</button>"), [[1, "button"]]);
  assert.deepEqual(bad('<button title="Close" onClick={go}><CloseIcon /></button>'), [[1, "button"]]);
  assert.deepEqual(bad('<button aria-label="Close" onClick={go}><CloseIcon /></button>'), [[1, "button"]]);
  assert.deepEqual(bad('<button class="x" title="Close" aria-label="Close"><CloseIcon size="sm" /></button>'), []);
  assert.deepEqual(bad('<button aria-label={`Close ${a > b ? "x" : "y"}`} title="Close"><Caret /></button>'), []);
  assert.deepEqual(bad("<button {...rest}><CloseIcon /></button>"), []);
  assert.deepEqual(bad("<button onClick={go}><CloseIcon /> Close</button>"), []);
  assert.deepEqual(bad("<button onClick={go}>Close</button>"), []);
  assert.deepEqual(bad('<IconButton label="Close"><CloseIcon /></IconButton>'), []);
});
