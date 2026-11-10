import assert from "node:assert/strict";
import { test } from "node:test";
import { offenders } from "./check-type.mjs";

const bad = (css) => offenders(css, "x.css").map((o) => [o.line, o.prop]);

test("a number, an em and a half-pixel are off the scale", () => {
  assert.deepEqual(bad(".a { font-size: 14px; }"), [[1, "font-size"]]);
  assert.deepEqual(bad(".a {\n  margin: 0;\n  font-size: 0.85em;\n}"), [[3, "font-size"]]);
  assert.deepEqual(bad(".a { font: 500 10.5px/1.3 var(--f-mono); }"), [[1, "font"]]);
  assert.deepEqual(bad(".a { font: 13px system-ui, sans-serif; }"), [[1, "font"]]);
  assert.deepEqual(bad(".a { letter-spacing: 0.04em; }"), [[1, "letter-spacing"]]);
});

test("the tokens, inherit and zero are fine", () => {
  assert.deepEqual(bad(".a { font-size: var(--text-md); letter-spacing: var(--track-caps); }"), []);
  assert.deepEqual(bad(".a { font: 800 var(--text-2xl)/0.95 var(--f-display); font-size: var(--t-field); }"), []);
  assert.deepEqual(bad(".a { font: inherit; font-size: inherit; letter-spacing: 0; }"), []);
  assert.deepEqual(bad(".a { font: italic 600 var(--text-sm) var(--f-ui); }"), []);
});

test("a marked declaration, the Doc's paper and a comment are exempt", () => {
  assert.deepEqual(bad(".a { font-size: 9px; /* off-scale: glyph */ }"), []);
  assert.deepEqual(bad("/* off-scale: glyph */\n.a { font-size: 9px; }"), []);
  assert.deepEqual(bad(".doc .md h1, .doc-heading { font: 700 22px/1.15 var(--f-display); }"), []);
  assert.deepEqual(bad("/* font-size: 14px */ .a { color: red; }"), []);
  assert.deepEqual(bad("@media (x) {\n  .a { font-size: 14px; }\n}"), [[2, "font-size"]]);
});
