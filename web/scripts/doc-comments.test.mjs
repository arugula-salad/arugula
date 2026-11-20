// A comment on a Doc (#1258): its anchor written into the message's text and
// read back out. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { commentOf, commentText, quoteOf, QUOTE_MAX } from "../src/shared/docs/doc-comment-text.ts";

const URL = "https://app.example/p/prj_1/docs/pdc_ab";

test("a comment's text links to its section, quotes the passage, then says its piece", () => {
  const t = commentText(URL, "Why", { section: "s2", quote: "Replies   queue\non the device." }, " Even photos? ");
  assert.equal(t, "[On “Why”](https://app.example/p/prj_1/docs/pdc_ab#s2)\n> Replies queue on the device.\n\nEven photos?");
  assert.deepEqual(commentOf("pdc_ab", t), { anchor: { section: "s2", quote: "Replies queue on the device." }, text: "Even photos?" });
});

test("the mention goes last, on its own line, and stays in what was said", () => {
  const t = commentText(URL, "Why", { section: "s2", quote: "" }, "Look", "@geek/docs-agent");
  assert.ok(t.endsWith("\n\nLook\n\n@geek/docs-agent"), t);
  assert.deepEqual(commentOf("pdc_ab", t), { anchor: { section: "s2", quote: "" }, text: "Look\n\n@geek/docs-agent" });
});

test("a page's address with slugs still names the Doc", () => {
  const t = commentText("https://app.example/p/prj_1-site/docs/pdc_ab-the-plan", "Why", { section: "s2", quote: "x" }, "y");
  assert.equal(commentOf("pdc_ab", t).anchor?.section, "s2");
  assert.equal(commentOf("pdc_a", t).anchor, null, "another Doc's id is no prefix of it");
});

test("a heading can't end the link early", () => {
  const t = commentText(URL, "Plan [draft]", { section: "s1", quote: "x" }, "y");
  assert.equal(commentOf("pdc_ab", t).anchor?.section, "s1");
});

test("another Doc's comment, or a plain message, has no anchor", () => {
  const t = commentText(URL, "Why", { section: "s2", quote: "x" }, "y");
  assert.equal(commentOf("pdc_cd", t).anchor, null);
  assert.deepEqual(commentOf("pdc_ab", "just a message"), { anchor: null, text: "just a message" });
});

test("a long quote is cut at a word, with an ellipsis", () => {
  const q = quoteOf("word ".repeat(200));
  assert.ok([...q].length <= QUOTE_MAX + 1, q.length);
  assert.ok(q.endsWith("word…"), q);
});
