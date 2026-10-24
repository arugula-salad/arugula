// Chat's emoji (#105) and code colours (#109): shortcodes turn into the
// character, a message of up to three emoji is "only emoji", the composer's
// `:ta` finds 🎉, and the bundled table holds the ones the issue names. Run
// with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import data from "../src/shared/chat/emoji-data.ts";
import { findEmoji, onlyEmoji, withEmoji } from "../src/shared/chat/emoji.ts";
import { highlight } from "../src/shared/ui/highlight.ts";

// The same parse as loadEmoji's, over the real data.
const cats = [];
const list = [];
const byName = new Map();
for (const line of data.split("\n")) {
  if (line.startsWith("# ")) cats.push(line.slice(2));
  else {
    const [char, names, ...words] = line.split(" ");
    const e = { char, names: names.split(","), words: words.join(" "), cat: cats.length - 1 };
    list.push(e);
    for (const n of e.names) byName.set(n, char);
  }
}
const table = { cats, list, byName };

test("the table has the shortcodes the issue names", () => {
  assert.equal(byName.get("joy"), "😂");
  assert.equal(byName.get("+1"), "👍");
  assert.equal(byName.get("tada"), "🎉");
  assert.ok(cats.length >= 8 && list.length > 1500);
});

test("shortcodes become emoji, unknown ones stay", () => {
  assert.equal(withEmoji("nice :tada: and :+1:", table), "nice 🎉 and 👍");
  assert.equal(withEmoji("at 10:30:45 or :nope:", table), "at 10:30:45 or :nope:");
  assert.equal(withEmoji(":JOY:", table), "😂");
});

test("up to three emoji and nothing else is large", () => {
  assert.ok(onlyEmoji("🎉", null));
  assert.ok(onlyEmoji("🎉 👍🏽 🇳🇴", null));
  assert.ok(onlyEmoji(":joy::joy:", table));
  assert.ok(!onlyEmoji(":joy:", null));
  assert.ok(!onlyEmoji("🎉🎉🎉🎉", null));
  assert.ok(!onlyEmoji("ok 🎉", null));
  assert.ok(!onlyEmoji("123", null));
  assert.ok(!onlyEmoji("", null));
});

test("typing :ta offers the party popper", () => {
  const found = findEmoji(table, "ta", 8).map((e) => e.char);
  assert.ok(found.includes("🎉"));
  // names that begin with it come before names that merely contain it
  assert.ok(findEmoji(table, "joy", 8)[0].names.some((n) => n.startsWith("joy")));
  assert.deepEqual(findEmoji(table, "", 8), []);
});

test("code gets comments, strings, numbers and keywords", () => {
  const p = highlight('const x = "a//b"; // hi\nreturn 42', "ts");
  assert.deepEqual(
    p.filter(([, c]) => c),
    [["const", "hl-k"], ['"a//b"', "hl-s"], ["// hi", "hl-c"], ["return", "hl-k"], ["42", "hl-n"]],
  );
  assert.equal(p.map(([s]) => s).join(""), 'const x = "a//b"; // hi\nreturn 42');
  assert.equal(highlight("x", "klingon"), null);
  assert.equal(highlight("SELECT 1", "sql")[0][1], "hl-k");
});
