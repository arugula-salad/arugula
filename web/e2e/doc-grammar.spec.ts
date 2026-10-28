// #964, #965, #254: the grammar of a Doc's charts, tiles, drawings and pictures, with no page. The
// daemon's validator (crates/arugulad-docs/src/docs/embeds.rs) and the web
// client's are run over the same files (crates/daemon/tests/fixtures/docs/),
// and then the chart is laid out: a bar a row, from zero, with axes, labels
// and a text alternative; a line a path a series.
//
// #256: the named colours' table (shared/docs/colours.json, which the
// daemon reads too) holds what style.css's `--doc-*` tokens hold, and a
// drawing's names become the scheme's values.

import { readFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { cssVar, HUES, hex, NAMES, resolve, usesNames } from "../src/shared/docs/colours";
import { buildChart, checkChart, checkSpec, niceTicks, parseChart, summary, type El, type Spec } from "../src/shared/ui/chart";
import { assetName, checkEmbeds, checkSvg, fences, imageRefs, plain, svgUrl } from "../src/shared/ui/embeds";
import { checkStats, statsSummary, toneOf } from "../src/shared/ui/stats";
import { example, examples, expected } from "./doc-examples";

const all = (el: El, tag: string): El[] => [...(el.tag === tag ? [el] : []), ...el.kids.flatMap((k) => all(k, tag))];
const texts = (el: El) => all(el, "text").map((t) => t.text!);
const chart = (name: string): Spec => {
  const p = parseChart(fences(example(name)).find((f) => f.lang === "chart")!.code);
  if ("why" in p) throw new Error(p.why);
  return p.spec;
};

test("every good example passes and reads as the text the daemon gives", () => {
  expect(examples()).toEqual(Object.keys(expected.good));
  for (const name of examples()) {
    expect(checkEmbeds(example(name), expected.assets), name).toBeNull();
    expect(plain(example(name)), name).toBe(expected.good[name]);
  }
});

test("every bad example is refused with a line naming the problem", () => {
  expect(examples("bad")).toEqual(Object.keys(expected.bad));
  for (const name of examples("bad")) {
    const why = checkEmbeds(example(name), expected.assets);
    expect(why, name).toContain(expected.bad[name]);
    expect(why, name).not.toContain("\n");
  }
});

test("a picture is checked wherever the page draws it, as the daemon does", () => {
  const check = (md: string) => checkEmbeds(md, expected.assets);
  expect(check("a ![x](assets/chart.png) b ![y](assets/photo.jpg)")).toBeNull();
  expect(check("![x](assets/chart.png) ![y](assets/gone.png)")).toMatch(/^image assets\/gone\.png: no such asset/);
  for (const [md, says] of [
    ["![x](assets/Chart.png)", "no such asset"],
    ["![x](assets/chart.PNG)", "must be one of"],
    ["![x](assets/chart)", "must be one of"],
    ["![x](assets/chart.png.html)", "must be one of"],
    ["![x](assets/.chart.png)", "must be assets/<file>"],
    ["![x](/assets/chart.png)", "must be assets/<file>"],
    ["![x](./assets/chart.png)", "must be assets/<file>"],
    ["![x](assets/../chart.png)", "must be assets/<file>"],
    ["![x]()", "must be assets/<file>"],
    ["![x](//example.com/a.png)", "must be assets/<file>"],
    ["**![x](assets/gone.png)**", "no such asset"],
    ["[![x](assets/gone.png)](https://example.com)", "no such asset"],
    ["1. one\n\n   - two\n\n     ![x](assets/gone.png)", "no such asset"],
    ["> ```\n> ![x](assets/gone.png)\n> ```", "no such asset"],
  ]) {
    const why = check(md);
    expect(why, md).toContain(says);
    expect(why, md).not.toContain("\n");
  }
  for (const md of [
    "[a link](https://example.com/a.png)",
    '![x](assets/gone.png "title")',
    "![x] (assets/gone.png)",
    "!\\[x](assets/gone.png)",
    "``![x](assets/gone.png)``",
    "`![x](assets/gone.png)` and ``more``",
    "```\n![x](assets/gone.png)\n```",
    "- item\n\n    ~~~\n    ![x](assets/gone.png)\n    ~~~",
    "text\n```\n![x](assets/gone.png)",
    "é `ü` ![ñ](assets/chart.png) 日本",
  ]) {
    expect(check(md), md).toBeNull();
  }
  expect(check("`a` ![x](assets/gone.png)")).not.toBeNull();
  // The text of a Doc says what a picture is, and leaves code as it was.
  expect(plain("See ![the  flow\nchart](assets/a.png) and ![x](assets/a.png), ![](assets/a.png) or ![y](http://e.com/a.png)")).toBe(
    "See ![the  flow\nchart](assets/a.png) and [image: x], [image] or ![y](http://e.com/a.png)",
  );
  // Only `assets/<plain name>.<picture extension>` is a picture the page draws.
  expect(["assets/a.png", "assets/a-b_c.JPG", "assets/a.jpeg", "assets/a.gif", "assets/a.webp", "assets/a.svg", "assets/a/b.png", "assets/.a.png", "http://a/b.png", `assets/${"a".repeat(97)}.png`, `assets/${"a".repeat(96)}.png`].map(assetName)).toEqual([
    "a.png",
    null,
    "a.jpeg",
    "a.gif",
    "a.webp",
    null,
    null,
    null,
    null,
    null,
    `${"a".repeat(96)}.png`,
  ]);
  expect(imageRefs("é `ü` ![ñ](assets/a.png)").map((r) => [r.alt, r.src])).toEqual([["ñ", "assets/a.png"]]);
});

test("only a real fence is an embed", () => {
  const md = example("code-not-embeds.md");
  expect(fences(md).map((f) => f.lang)).toEqual(["text", "json"]);
  expect(checkEmbeds(md)).toBeNull();
  // The language is the first word; the rest of the line is the alt text.
  expect(fences("```svg A box, drawn\n<svg/>\n```").map((f) => [f.lang, f.alt])).toEqual([["svg", "A box, drawn"]]);
  expect(fences("~~~chart\n{}\n~~~").map((f) => f.lang)).toEqual(["chart"]);
  expect(fences("text\n```chart\n{").map((f) => f.code)).toEqual(["{"]);
});

test("the rules the daemon's table has: series, rows, numbers, ordering", () => {
  const rows = (n: number) => Array.from({ length: n }, (_, i) => ({ m: i, a: 1, b: 2 }));
  const bar = (y: unknown, r: unknown) => ({ type: "bar", x: "m", y, rows: r });
  const line = (r: unknown[], more = {}) => ({ type: "line", x: "m", y: "a", rows: r, ...more });
  for (const [spec, says] of [
    [bar(["a", "b", "a", "b", "a"], rows(2)), "1 to 4 keys"],
    [bar([], rows(2)), "1 to 4 keys"],
    [bar("a", []), "non-empty list"],
    [bar("a", rows(201)), "201 rows; the cap is 200"],
    [bar("a", [{ m: "x", a: "7" }]), "must be a number"],
    [bar("a", [{ m: "x", a: null }]), "must be a number"],
    [bar("a", [{ a: 1 }]), 'has no "m" label'],
    [bar("a", [3]), "row 1 must be an object"],
    [{ ...bar("a", rows(1)), title: 4 }, "title must be a string"],
    [{ type: "bar", y: "a", rows: rows(1) }, "x must name"],
    [[1], "must be a JSON object"],
    [line([{ m: 1, a: 1 }, { m: 1, a: 2 }]), "x must go up: row 2 (1)"],
    [line([{ m: "2026-02-01", a: 1 }, { m: "2026-01-31", a: 2 }]), "x must go up"],
    [line([{ m: "2026-01-31T10:00:00Z", a: 1 }, { m: "2026-01-31T10:00:00+02:00", a: 2 }]), "x must go up"],
    [line([{ m: "b", a: 1 }, { m: "a", a: 2 }]), "must be numbers, dates"],
    [line([{ m: "b", a: 1 }, { m: "a", a: 2 }], { order: ["a", "b"] }), "x must go up"],
    [line([{ m: "a", a: 1 }, { m: "b", a: 2 }], { order: ["a"] }), 'row 2 ("b") is not in order'],
    [line([{ m: "a", a: 1 }], { order: "ab" }), "order must be a list"],
  ] as [unknown, string][]) {
    expect(checkSpec(spec), JSON.stringify(spec)).toContain(says);
  }
  const ok = (spec: unknown) => expect(checkSpec(spec), JSON.stringify(spec)).toBeNull();
  ok(bar(["a", "b", "a", "b"], rows(200)));
  ok(bar("a", [{ m: "z", a: -3 }, { m: "b", a: 0.5 }]));
  ok(line([{ m: "2026-01-31", a: 1 }, { m: "2026-02-01 08:00", a: 2 }, { m: "2026-02-01T10:00:00+01:00", a: 3 }]));
  ok(line([{ m: -2, a: 1 }, { m: 0.5, a: 2 }]));
  // A date that isn't one is a label, as the daemon takes it.
  ok(line([{ m: "2026-13-01", a: 1 }, { m: "2026-13-02", a: 2 }], { order: ["2026-13-01", "2026-13-02"] }));
  expect(checkChart("{ not json")).toMatch(/^chart: not JSON \(/);
});

test("a chart says what it is as a line of text", () => {
  expect(summary('{"type":"bar","title":"Sales\\nby month","x":"m","y":"a","rows":[{"m":1,"a":2}]}')).toBe("[chart: Sales by month (bar, 1 row)]");
  expect(summary("nope")).toBe("[chart]");
});

test("a drawing needs an <svg> root with its namespace, and stays under the cap", () => {
  const ns = 'xmlns="http://www.w3.org/2000/svg"';
  for (const good of [`<svg ${ns}><rect/></svg>`, `<?xml version="1.0"?>\n<!-- a box -->\n<svg ${ns}/>`, `<svg xmlns='http://www.w3.org/2000/svg'/>`, `<svg ${ns}><script>alert(1)</script></svg>`]) {
    expect(checkSvg(good), good).toBeNull();
  }
  for (const [bad, says] of [
    ["", "empty"],
    ["<div>no</div>", "start with <svg"],
    ["<svgfoo/>", "start with <svg"],
    ["<svg viewBox='0 0 1 1'></svg>", "needs xmlns"],
    [`<svg><g ${ns}/></svg>`, "needs xmlns"],
    [`<svg ${ns}>${"x".repeat(64 * 1024)}</svg>`, "the cap is 65536"],
  ]) {
    expect(checkSvg(bad), bad.slice(0, 40)).toContain(says);
  }
  // The picture's URL is a data: URL, which control's CSP allows.
  expect(svgUrl("<svg a='b'/>")).toBe("data:image/svg+xml;charset=utf-8,%3Csvg%20a%3D'b'%2F%3E");
});

test("a bar chart has a bar per row, from zero, with axes, labels and a text alternative", () => {
  const svg = buildChart(chart("bar-chart.md")); // Jan 12, Feb 18, Mar 25
  expect(svg.attrs.role).toBe("img");
  expect(svg.attrs["aria-label"]).toBe("Sales by month, bar chart. sales: Jan 12, Feb 18, Mar 25");
  expect(svg.kids[0].tag).toBe("title");
  expect(svg.kids[0].text).toBe(svg.attrs["aria-label"]);
  const bars = all(svg, "rect");
  expect(bars).toHaveLength(3);
  const base = Number(bars[0].attrs.y) + Number(bars[0].attrs.height);
  for (const b of bars) expect(Number(b.attrs.y) + Number(b.attrs.height)).toBeCloseTo(base, 9);
  const h = bars.map((b) => Number(b.attrs.height));
  expect(h[0] / h[2]).toBeCloseTo(12 / 25, 9);
  for (const wanted of ["Sales by month", "Jan", "Feb", "Mar", "month", "sales", "0"]) expect(texts(svg), wanted).toContain(wanted);
});

test("a line chart with two series has a path each, dots, a legend; text is currentColor and series are theme colours", () => {
  const svg = buildChart(chart("line-chart.md"));
  expect(svg.attrs["aria-label"]).toBe("Sign-ups by month, line chart. web: Jan 4, Feb 7, Mar 6, Apr 11. phone: Jan 2, Feb 5, Mar 9, Apr 12");
  const paths = all(svg, "path");
  expect(paths).toHaveLength(2);
  expect(paths[0].attrs.d).toMatch(/^M[\d.]+ [\d.]+( L[\d.]+ [\d.]+){3}$/);
  expect(paths[0].attrs.style).not.toBe(paths[1].attrs.style);
  expect(paths[0].attrs.style).toMatch(/var\(--c1, #/);
  expect(paths[1].attrs.style).toMatch(/var\(--c2, #/);
  expect(all(svg, "circle")).toHaveLength(8);
  expect(texts(svg)).toEqual(expect.arrayContaining(["Sign-ups by month", "web", "phone"]));
  expect(all(svg, "text").every((t) => t.attrs.fill === "currentColor")).toBe(true);
  expect(all(svg, "line").every((l) => l.attrs.stroke === "currentColor")).toBe(true);
});

test("a line over dates draws, long labels thin out, and 200 rows draw", () => {
  expect(all(buildChart(chart("line-dates.md")), "circle")).toHaveLength(3);
  const rows = Array.from({ length: 12 }, (_, i) => ({ m: `Month ${i + 1}`, v: i }));
  const labels = texts(buildChart({ type: "bar", x: "m", y: "v", rows })).filter((t) => t.startsWith("Month"));
  expect(labels.length).toBeGreaterThan(0);
  expect(labels.length).toBeLessThan(12);
  const big = Array.from({ length: 200 }, (_, i) => ({ x: i, a: i, b: 2 * i, c: 3 * i, d: 4 * i }));
  const line = buildChart({ type: "line", x: "x", y: ["a", "b", "c", "d"], rows: big });
  expect(all(line, "path")).toHaveLength(4);
  expect(all(line, "circle"), "no dots on a long series").toHaveLength(0);
  expect(niceTicks(0, 25)).toEqual([0, 10, 20, 30]);
});

// The daemon's each_stats_rule_is_enforced, in TypeScript.
test("each stats rule is enforced, as the daemon says it", () => {
  const tile = { label: "Sessions", value: "1,284", delta: "+12%", tone: "up" };
  for (const [tiles, says] of [
    [[], "0 tiles; a stats fence holds 1 to 6"],
    [Array(7).fill(tile), "7 tiles; a stats fence holds 1 to 6"],
    [tile, "must be a JSON list of 1 to 6 tiles"],
    [["Sessions"], "tile 1 must be an object"],
    [[tile, { value: "3" }], "tile 2 has no label"],
    [[{ label: "Errors" }], "tile 1 has no value"],
    [[{ label: "Errors", value: 3 }], 'value must be text ("1,284", not 1284)'],
    [[{ label: " ", value: "3" }], "label must be text"],
    [[{ label: "x".repeat(41), value: "3" }], "at most 40 characters"],
    [[{ label: "Errors", value: "3", delta: -2 }], "delta must be text"],
    [[{ label: "Errors", value: "3", tone: "up" }], "tone needs a delta"],
    [[{ label: "Errors", value: "3", delta: "-2", tone: "red" }], 'tone must be "up", "down" or "flat", got "red"'],
    [[{ label: "Errors", value: "3", color: "leaf" }], 'unknown key "color"'],
  ] as const) {
    const why = checkStats(JSON.stringify(tiles));
    expect(why, JSON.stringify(tiles)).toMatch(/^stats: /);
    expect(why, JSON.stringify(tiles)).toContain(says);
    expect(why).not.toContain("\n");
  }
  expect(checkStats("nope")).toMatch(/^stats: not JSON \(/);
  expect(checkStats(JSON.stringify(Array(6).fill(tile)))).toBeNull();
  // Caps are characters, not UTF-16 units: an emoji is one.
  expect(checkStats(JSON.stringify([{ label: "é".repeat(40), value: "📈".repeat(24), delta: "↑".repeat(16), tone: "flat" }]))).toBeNull();
  expect(statsSummary('[{"label": "Sessions", "value": "1,284", "delta": "+12%"}, {"label": "Errors", "value": "3"}]')).toBe("[stats: Sessions 1,284 (+12%); Errors 3]");
  expect(statsSummary("nope")).toBe("[stats]");
  // A delta with no tone takes its sign's; the arrow is drawn either way.
  expect(["+3", "-3", "−3", "3"].map((delta) => toneOf({ label: "a", value: "1", delta }))).toEqual(["up", "down", "down", "flat"]);
  expect(toneOf({ label: "a", value: "1", delta: "+3", tone: "down" })).toBe("down");
});

/** The custom properties a rule in style.css sets, by its selector. */
function tokens(selector: string): Record<string, string> {
  const css = readFileSync(new URL("../src/shared/style.css", import.meta.url), "utf8");
  const at = css.indexOf(`\n${selector} {\n`);
  expect(at, selector).toBeGreaterThan(-1);
  const body = css.slice(at, css.indexOf("\n}", at));
  return Object.fromEntries([...body.matchAll(/^\s*(--[\w-]+):\s*([^;]+);/gm)].map((m) => [m[1], m[2].trim()]));
}

test("the named colours are style.css's Doc tokens, in both schemes", () => {
  expect(NAMES).toHaveLength(16);
  expect(HUES).toEqual(["leaf", "pepper", "sky", "plum", "sun", "stone"]);
  const dark = tokens(".doc");
  const light = { ...dark, ...tokens('html[data-scheme="light"] .doc') };
  for (const name of NAMES) {
    expect(dark[cssVar(name)], `${name} dark`).toBe(hex(name, "dark"));
    expect(light[cssVar(name)], `${name} light`).toBe(hex(name, "light"));
  }
  // A chart's default series are the first four hues.
  expect(["--c1", "--c2", "--c3", "--c4"].map((c) => dark[c])).toEqual(HUES.slice(0, 4).map((h) => `var(--doc-${h})`));
});

test("a drawing's names become the scheme's values; a hex drawing stays as it was", () => {
  const named = fences(example("named-drawing.md"))[0].code;
  expect(usesNames(named)).toBe(true);
  for (const scheme of ["light", "dark"] as const) {
    const out = resolve(named, scheme);
    expect(out).toContain(`fill="${hex("leaf-soft", scheme)}" stroke="${hex("leaf", scheme)}"`);
    expect(out).toContain(`stroke="${hex("ink", scheme)}"`);
    // What it doesn't paint, like the second label, is ink.
    expect(out).toMatch(new RegExp(`^<svg fill="${hex("ink", scheme)}" xmlns=`));
    expect(out).not.toMatch(/"(?:leaf|stone|ink)(?:-soft)?"/);
  }
  expect(resolve(named, "light")).not.toBe(resolve(named, "dark"));
  // A root with a fill of its own keeps it, and a name in text is text.
  const own = `<svg xmlns="http://www.w3.org/2000/svg" fill='sky'><text>leaf</text></svg>`;
  expect(resolve(own, "dark")).toBe(`<svg xmlns="http://www.w3.org/2000/svg" fill='${hex("sky", "dark")}'><text>leaf</text></svg>`);
  const old = fences(example("drawing.md"))[0].code;
  expect(usesNames(old)).toBe(false);
  // Bad words, as the daemon says them.
  for (const [bad, says] of [
    [`<svg xmlns="http://www.w3.org/2000/svg"><rect fill="leafy"/></svg>`, 'fill="leafy" is not a colour'],
    [`<svg xmlns="http://www.w3.org/2000/svg"><rect stroke='green-soft'/></svg>`, 'stroke="green-soft" is not a colour'],
    [`<svg xmlns="http://www.w3.org/2000/svg"><stop stop-color="#12"/></svg>`, 'stop-color="#12" is not a colour'],
  ]) {
    expect(checkSvg(bad), bad).toContain(says);
  }
  for (const good of [`<rect fill="none" stroke="currentColor" color=" Leaf "/>`, `<rect fill="url(#g)" stroke="rgb(1, 2, 3)"/><text fill="white">plum</text>`, `<rect fill-opacity="0.5" fill=""/>`]) {
    expect(checkSvg(`<svg xmlns="http://www.w3.org/2000/svg">${good}</svg>`), good).toBeNull();
  }
});

test("a chart's colors name a hue for each series; without them, the brand order", () => {
  const svg = buildChart(chart("chart-colours.md"));
  const paths = all(svg, "path");
  expect(paths[0].attrs.style).toBe(`stroke:var(--doc-leaf, ${hex("leaf", "light")})`);
  expect(paths[1].attrs.style).toBe(`stroke:var(--doc-stone, ${hex("stone", "light")})`);
  for (const [colors, says] of [
    [["leaf"], "colors must name one colour for each y series, from leaf, pepper, sky, plum, sun, stone"],
    ["leaf", "one colour for each y series"],
    [["leaf", "ink"], 'colors: "ink" is not one of'],
    [["leaf-soft", "sky"], 'colors: "leaf-soft" is not one of'],
  ] as const) {
    expect(checkSpec({ type: "bar", x: "m", y: ["a", "b"], colors, rows: [{ m: 1, a: 1, b: 2 }] })).toContain(says);
  }
});
