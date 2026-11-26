import assert from "node:assert/strict";
import { test } from "node:test";
import { EMBEDDED, fieldClasses, offenders } from "./check-fields.mjs";

const bad = (css, file = "x.css", classes) => offenders(css, file, classes).map((o) => [o.line, o.prop]);

test("a place rule that draws a field fails", () => {
  assert.deepEqual(bad(".a input { border: 1px solid var(--line); }"), [[1, "border"]]);
  assert.deepEqual(bad(".a select {\n  width: 100%;\n  background-color: var(--letterbox);\n}"), [[3, "background-color"]]);
  assert.deepEqual(bad(".a input:not([type=\"checkbox\"]) { height: 28px; }"), [[1, "height"]]);
  assert.deepEqual(bad(".a input:not(.sw-switch, [type=\"range\"]) { height: 20px; }"), [[1, "height"]]);
  assert.deepEqual(bad(".a textarea:focus { box-shadow: none; border-color: var(--accent); }"), [[1, "box-shadow"], [1, "border-color"]]);
  assert.deepEqual(bad(".a :is(input, textarea) { background: var(--surface); }"), [[1, "background"]]);
  assert.deepEqual(bad("@media (x) {\n  .b, .a > select { min-height: 40px; }\n}"), [[2, "min-height"]]);
});

test("placing a field, and what isn't one, is fine", () => {
  assert.deepEqual(bad(".a input { flex: 1; width: 100%; margin: 0; font-family: var(--f-mono); border-radius: 0; }"), []);
  assert.deepEqual(bad(".a input[type=\"range\"] { background: none; height: 4px; }"), []);
  assert.deepEqual(bad(".a input::placeholder { color: var(--dim); } .a input::-webkit-slider-thumb { background: red; }"), []);
  assert.deepEqual(bad(".a input.sw-switch { background: var(--line); }"), []);
  assert.deepEqual(bad(".a:has(input) { border: 1px solid; } .a input + label { border: 0; }"), []);
  assert.deepEqual(bad("/* .a input { border: 0 } */ .a { color: red; }"), []);
});

test("a textarea keeps its height, not its look", () => {
  assert.deepEqual(bad(".a textarea { min-height: 8em; height: auto; background: none; }"), [[1, "background"]]);
  assert.deepEqual(bad(".a input, .a textarea { min-height: 8em; }"), [[1, "min-height"]]);
});

test("the kit's files are its own", () => {
  assert.deepEqual(bad(":where(input) { border: 1px solid; }", "shared/fields.css"), []);
  assert.deepEqual(bad(".search-field > input { border: 0; }", "shared/ui/search-field.css"), []);
});

test("an embedded box is allowed by its file and selector, nothing else", () => {
  const [file, sels] = [...EMBEDDED][0];
  const sel = [...sels][0];
  assert.deepEqual(bad(`${sel} { border: 0; background: none; }`, file), []);
  assert.deepEqual(bad(`${sel} { border: 0; }`, "elsewhere.css").length, 1);
  assert.deepEqual(bad(`${sel}, .a input { border: 0; }`, file), [[1, "border"]]);
});

test("a class only ever on a field is read as one", () => {
  const classes = fieldClasses([
    `<input class="find-box" />`,
    `<textarea class={"note-box"} rows={3} onKeyDown={(e) => e.key === ">" && go()} />`,
    `<div class="row"><input class="row" /></div>`,
    `<select class={\`pick \${size}\`}>`,
    `<input class="field small" />`,
    `const answer = document.createElement("textarea");\nanswer.rows = 3;\nanswer.className = "answer-box";`,
  ]);
  assert.deepEqual([...classes].sort(), [["answer-box", "textarea"], ["find-box", "field"], ["note-box", "textarea"], ["pick", "field"]]);
  assert.deepEqual(bad(".find-box { background: var(--letterbox); }", "x.css", classes), [[1, "background"]]);
  assert.deepEqual(bad(".note-box { min-height: 9em; border: 0; }", "x.css", classes), [[1, "border"]]);
  assert.deepEqual(bad(".row { border: 1px solid; } .field { min-height: 40px; }", "x.css", classes), []);
});

test("the kit may draw a field", () => {
  assert.deepEqual(bad(".k-field-box input { border: 0; background: none; }", "shared/kit/kit.css"), []);
});
