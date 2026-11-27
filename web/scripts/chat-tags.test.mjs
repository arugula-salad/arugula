// Tags in the agents' rooms (Arugula #1522): the `@` list, the tags a post
// sends by id, a tag drawn by its board's name now, and the receipts.
// Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { composerLine, receipt, tagAt, tagOptions, tagsIn } from "../src/shared/chat/tags.ts";

const boards = [
  { id: "brd_1", title: "Design lift", lead: true },
  { id: "brd_2", title: "A2P", lead: true },
  { id: "brd_3", title: "A2P follow-ups", lead: false },
];
const title = (id) => boards.find((b) => b.id === id)?.title;

test("the @ list: channel, here, cos, then each board's lead; any word of a board's name matches", () => {
  assert.deepEqual(tagOptions("agents", boards, "").map((o) => o.tag.text), ["channel", "here", "cos", "Design lift", "A2P", "A2P follow-ups"]);
  assert.deepEqual(tagOptions("agents", boards, "li").map((o) => o.tag.text), ["Design lift"]);
  assert.equal(tagOptions("agents", boards, "").find((o) => o.tag.text === "A2P follow-ups").note, "no lead yet");
  assert.equal(tagOptions("board", boards, "here")[0].note, "its active workers");
});

test("a post's tags are by id, the longest name first, at word edges, once each", () => {
  const all = tagOptions("agents", boards, "");
  assert.deepEqual(tagsIn("@A2P follow-ups and @Design lift, then @a2p again; me@here no", all), [
    { kind: "board", board: "brd_3", text: "A2P follow-ups" },
    { kind: "board", board: "brd_1", text: "Design lift" },
    { kind: "board", board: "brd_2", text: "A2P" },
  ]);
  assert.deepEqual(tagsIn("@here @cos", all).map((t) => t.kind), ["here", "cos"]);
  assert.deepEqual(tagsIn("@heresy", all), []);
});

test("a tag is drawn by its board's name now: a rename keeps it", () => {
  const tags = [{ kind: "board", board: "brd_1", text: "Old name" }];
  assert.deepEqual(tagAt("@Old name, merge", tags, title), { len: 9, kind: "board", name: "Design lift" });
  assert.equal(tagAt("@Old names", tags, title), null);
});

test("receipts say who a post reached", () => {
  const seven = ["a", "b", "c", "d", "e", "f", "g"];
  assert.equal(receipt({ leads: seven, all: true }, title), "reached: 7 leads");
  assert.equal(receipt({ leads: ["brd_1", "brd_2"] }, title), "reached: Design lift, A2P");
  assert.equal(receipt({ leads: ["a", "b", "c", "d"], active: true }, title), "reached: 4 active leads");
  assert.equal(receipt({ leads: [] }, title), "reached: no lead · it tagged none");
  assert.equal(receipt({ leads: seven, all: true, cos: true }, title), "reached: 7 leads, chief of staff");
  assert.equal(receipt({ leads: ["brd_1"], workers: 2 }, title), "reached: Design lift, 2 workers");
});

test("the composer's line", () => {
  assert.equal(composerLine("agents", false), "Every lead gets this, no tag needed");
  assert.equal(composerLine("board", false), "Its lead gets this · @here adds active workers");
  assert.equal(composerLine("agents", true), "Replies reach whoever the post reached");
});
