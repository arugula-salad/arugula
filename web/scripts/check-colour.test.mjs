import assert from "node:assert/strict";
import { test } from "node:test";
import { offenders } from "./check-colour.mjs";

const bad = (css, file = "shared/x.css") => offenders(css, file).map((o) => [o.line, o.prop]);

test("a hex colour of any length fails", () => {
  assert.deepEqual(bad(".a { color: #fff; }"), [[1, "color"]]);
  assert.deepEqual(bad(".a { color: #abcd; }"), [[1, "color"]]);
  assert.deepEqual(bad(".a {\n  margin: 0;\n  background: #1e1e2e;\n}"), [[3, "background"]]);
  assert.deepEqual(bad(".a { box-shadow: 0 2px 8px #000000aa; }"), [[1, "box-shadow"]]);
  assert.deepEqual(bad(".a { background: var(--x, #e5484d); }"), [[1, "background"]]);
});

test("rgb(), rgba(), hsl() and hsla() fail", () => {
  assert.deepEqual(bad(".a { color: rgb(1 2 3); }"), [[1, "color"]]);
  assert.deepEqual(bad(".a { color: rgba(1, 2, 3, 0.5); }"), [[1, "color"]]);
  assert.deepEqual(bad(".a { color: hsl(10 20% 30%); }"), [[1, "color"]]);
  assert.deepEqual(bad(".a { color: HSLA(10, 20%, 30%, 0.5); }"), [[1, "color"]]);
  assert.deepEqual(bad(".a { box-shadow: 0 0 0 1px color-mix(in srgb, red, rgb(0 0 0 / 0.5)); }"), [[1, "box-shadow"]]);
});

test("a token with hex digits after it fails", () => {
  assert.deepEqual(bad(".a { background: var(--accent)22; }"), [[1, "background"]]);
  assert.deepEqual(bad(".a { border: 1px solid var(--info)66; }"), [[1, "border"]]);
  assert.deepEqual(bad("html[data-finish=\"x\"] .a { background: var(--accent)22; }"), [[1, "background"]]);
  assert.deepEqual(bad(":root { --tint: var(--accent)22; }", "shared/style.css"), [[1, "--tint"]]);
});

test("tokens, color-mix, keywords, sizes and url() ids are fine", () => {
  assert.deepEqual(bad(".a { color: var(--fg); background: color-mix(in srgb, var(--accent) 13%, transparent); }"), []);
  assert.deepEqual(bad(".a { box-shadow: var(--shadow-md); border: 1px solid var(--line, currentColor); }"), []);
  assert.deepEqual(bad(".a { background: transparent; margin: var(--space-1)10px; width: calc(var(--w) * 2); }"), []);
  assert.deepEqual(bad(".a { fill: url(#abc); background: url(\"data:image/svg+xml;utf8,<svg fill='%23fff'/>\"); }"), []);
});

test("the tokens' own definitions are allowed in shared/style.css's :root and html[data-…] rules only", () => {
  const defs = ":root { --bg: #1e1e2e; --shadow-sm: 0 2px 8px rgb(0 0 0 / 0.4); }\nhtml[data-scheme=\"light\"] { --bg: #eff1f5; }";
  assert.deepEqual(bad(defs, "shared/style.css"), []);
  assert.deepEqual(bad("@media (pointer: coarse) {\n  :root { --bg: #000; }\n}", "shared/style.css"), []);
  assert.deepEqual(bad(defs, "shared/other.css"), [[1, "--bg"], [1, "--shadow-sm"], [2, "--bg"]]);
  assert.deepEqual(bad(".swarm { --void: #000; }", "shared/style.css"), [[1, "--void"]]);
  assert.deepEqual(bad(":root .a, .b { --x: #000; }", "shared/style.css"), [[1, "--x"]]);
  assert.deepEqual(bad(":root { color: #000; }", "shared/style.css"), [[1, "color"]]);
});

test("the finish may use white and black as an alpha base, and nothing else", () => {
  const rule = (sel, v) => `${sel} { box-shadow: ${v}; }`;
  assert.deepEqual(bad(rule('html[data-finish="aqua"] .a', "inset 0 1px 0 #fff8, 0 1px 3px rgb(0 0 0 / 0.3)")), []);
  assert.deepEqual(bad(rule('[data-look="glass"] .a', "0 0 0 1px rgb(255 255 255 / 0.2)")), []);
  assert.deepEqual(bad(rule('html[data-finish="aqua"] .a, html[data-finish="glass"] .b', "0 0 0 1px #000")), []);
  assert.deepEqual(bad("a { background: linear-gradient(#fff3, #0000); }", "platform/finish.css"), []);
  assert.deepEqual(bad("a { background: rgb(255 255 255 / 0.1); }", "shared/buttons.css"), []);
  assert.deepEqual(bad(rule('html[data-finish="aqua"] .a', "0 1px 3px #3366ff")), [[1, "box-shadow"]]);
  assert.deepEqual(bad(rule('html[data-finish="aqua"] .a', "0 1px 3px rgb(51 102 255 / 0.5)")), [[1, "box-shadow"]]);
  assert.deepEqual(bad(rule('html[data-finish="aqua"] .a', "0 1px 3px rgb(0 0 0)")), [[1, "box-shadow"]]);
  assert.deepEqual(bad("a { color: #3366ff; }", "platform/finish.css"), [[1, "color"]]);
  assert.deepEqual(bad("a { color: #123; }", "shared/buttons.css"), [[1, "color"]]);
  assert.deepEqual(bad(rule('html[data-finish="aqua"] .a, .b', "0 0 0 1px #fff8")), [[1, "box-shadow"]]);
  assert.deepEqual(bad(rule(".a", "0 0 0 1px #fff8")), [[1, "box-shadow"]]);
});

test("a mask's #000 is an alpha mask, not a colour", () => {
  assert.deepEqual(bad(".a { mask: linear-gradient(#000, transparent); -webkit-mask-image: linear-gradient(#000 0 0); mask-image: linear-gradient(#000, #0000); }"), []);
  assert.deepEqual(bad(".a { -webkit-mask-composite: xor; background: linear-gradient(#000, #0000); }"), [[1, "background"]]);
});

test("a marked line, with a reason, is exempt", () => {
  assert.deepEqual(bad(".a { background: #fff; /* lint: literal — a page paints on white */ }"), []);
  assert.deepEqual(bad("/* lint: literal — a camera frame is black */\n.a { background: #000; }"), []);
  assert.deepEqual(bad(".a {\n  /* lint: literal — a page paints on white */\n  background: #fff;\n  color: #000;\n}"), [[4, "color"]]);
  assert.deepEqual(bad(".a { background: #fff; /* lint: literal - ascii dash is fine */ }"), []);
});

test("a mark with no reason does not exempt", () => {
  assert.deepEqual(bad(".a { background: #fff; /* lint: literal */ }"), [[1, "background"]]);
  assert.deepEqual(bad(".a { background: #fff; /* lint: literal — */ }"), [[1, "background"]]);
  assert.deepEqual(bad("/* lint: literal */\n.a { background: #fff; }"), [[2, "background"]]);
});

test("a comment is not read, and a rule inside @media is", () => {
  assert.deepEqual(bad("/* color: #fff */ .a { color: red; }"), []);
  assert.deepEqual(bad("@media (x) {\n  .a { color: #fff; }\n}"), [[2, "color"]]);
});
