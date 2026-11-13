// A Doc downloaded as Markdown (Arugula #865): its title, each section's
// heading and body, and its pictures left out with a closing line that says so.
// Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { docMarkdown, fileName, PICTURES_LEFT_OUT } from "../src/shared/docs/download.ts";

const section = (heading, body) => ({ id: heading, heading, intent: "", body, rev: 1 });

test("the title, then each section's heading and body; a pending section is its heading", () => {
  const md = docMarkdown({ doc: { title: "Plan" }, sections: [section("Why", "Because.\n"), section("Later", null), section("How", "- one\n- two")], assets: [] });
  assert.equal(md, "# Plan\n\n## Why\n\nBecause.\n\n## Later\n\n## How\n\n- one\n- two\n");
  assert.ok(!md.includes(PICTURES_LEFT_OUT), "no pictures, no closing line");
});

test("its pictures are left out, and the file says so once, at the end", () => {
  const md = docMarkdown({
    doc: { title: "Chart" },
    sections: [section("One", "Before ![a chart](assets/chart.png) after\n\n![x](https://example.com/x.png)"), section("Two", "![b](assets/b.png)")],
    assets: [{ name: "chart.png", mime: "image/png", size: 1, rev: 1 }],
  });
  assert.ok(!md.includes("assets/"), md);
  assert.ok(md.includes("![x](https://example.com/x.png)"), "a picture from elsewhere is a link, kept");
  assert.equal(md.split(PICTURES_LEFT_OUT).length, 2);
  assert.ok(md.endsWith(`${PICTURES_LEFT_OUT}\n`));
  assert.ok(md.includes("## Two\n\n" + PICTURES_LEFT_OUT), "a section of a picture alone is its heading");
});

test("the file is named for the title", () => {
  assert.equal(fileName("Q3: plan / notes"), "Q3 plan notes.md");
  assert.equal(fileName("  "), "Doc.md");
});
