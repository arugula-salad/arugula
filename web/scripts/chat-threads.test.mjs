// Chat's reply threads (#106), the plain parts: how a parent says it has
// replies, where the New line goes, a thread read or followed before the
// server has answered, and the paths a thread's links take. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ago, badge, followed, footer, mergeReplies, newFrom, readTo, repliesOf, rootOf } from "../src/shared/chat/threads.ts";
import { parse, pathOf } from "../src/platform/paths.ts";

const at = Date.parse("2026-10-24T12:00:00Z");
const thread = (o = {}) => ({ replies: 3, last: at - 5 * 60_000, repliers: [], following: true, read: 10, unread: 2, mentions: 0, ...o });
const reply = (seq, author = "b", extra = {}) => ({ seq, author, deleted: null, parent: 5, ...extra });
const mine = (m) => m.author === "a";

test("a parent says how many replies and how long ago", () => {
  assert.equal(repliesOf(1), "1 reply");
  assert.equal(repliesOf(12), "12 replies");
  assert.equal(ago(at - 10_000, at), "just now");
  assert.equal(ago(at - 5 * 60_000, at), "5m ago");
  assert.equal(ago(at - 3 * 3600_000, at), "3h ago");
  assert.equal(ago(at - 2 * 86400_000, at), "2d ago");
  assert.equal(footer(thread(), at), "3 replies · last reply 5m ago");
  assert.equal(footer(thread({ replies: 1, last: at - 1000 }), at), "1 reply · last reply just now");
});

test("a message is in the thread it starts or the one it replies in", () => {
  assert.equal(rootOf({ seq: 5 }), 5);
  assert.equal(rootOf({ seq: 9, parent: 5 }), 5);
});

test("the New line goes before the first reply of someone else's after the mark", () => {
  const rs = [reply(11, "a"), reply(12), reply(13, "a"), reply(14)];
  assert.equal(newFrom(rs, 10, mine), 12);
  assert.equal(newFrom(rs, 12, mine), 14);
  assert.equal(newFrom(rs, 14, mine), null);
  assert.equal(newFrom([reply(12, "b", { deleted: 1 })], 10, mine), null, "a deleted one is nothing new");
});

test("reading a thread shows at once: forward only, unread from what's after", () => {
  const m = { seq: 5, thread: thread({ unread: 2, mentions: 1 }) };
  const rs = [reply(11), reply(12), reply(13, "a")];
  const half = readTo(m, 11, rs, mine);
  assert.deepEqual([half.thread.read, half.thread.unread], [11, 1]);
  const all = readTo(m, 13, rs, mine);
  assert.deepEqual([all.thread.read, all.thread.unread, all.thread.mentions], [13, 0, 0]);
  assert.equal(readTo(m, 8, rs, mine), m, "never back");
  const not = readTo({ seq: 5, thread: thread({ following: false, unread: 0 }) }, 13, rs, mine);
  assert.equal(not.thread.unread, 0, "a thread I don't follow has no unread");
});

test("following begins from now; stopping leaves nothing unread", () => {
  const m = { seq: 5, thread: thread({ following: false, unread: 0, read: 3 }) };
  const on = followed(m, true, 12);
  assert.deepEqual([on.thread.following, on.thread.read, on.thread.unread], [true, 12, 0]);
  const off = followed({ seq: 5, thread: thread() }, false, 12);
  assert.deepEqual([off.thread.following, off.thread.unread], [false, 0]);
  assert.equal(followed(m, false, 12), m, "already so");
});

test("two runs of replies are one, by number, the later copy winning", () => {
  const a = [reply(11, "b", { text: "old" }), reply(12)];
  const b = [reply(11, "b", { text: "new" }), reply(13)];
  const m = mergeReplies(a, b);
  assert.deepEqual(m.map((x) => x.seq), [11, 12, 13]);
  assert.equal(m[0].text, "new");
});

test("the Threads badge counts unread replies, @ when some mention me", () => {
  assert.equal(badge(0, 0), "");
  assert.equal(badge(3, 0), "3");
  assert.equal(badge(3, 1), "@3");
});

test("a reply's link names its thread, and the Threads view has a path", () => {
  const route = { page: "chat", project: "prj_8f2ec0ffee00112233445566778899aa", room: "ch_00112233", msg: 9, thread: 5 };
  const path = pathOf(route);
  assert.equal(path, "/p/prj_8f2ec0ffee00112233445566778899aa/chat/ch_00112233?msg=9&thread=5");
  assert.deepEqual(parse("/p/prj_8f2ec0ffee00112233445566778899aa/chat/ch_00112233", "?msg=9&thread=5"), route);
  assert.deepEqual(parse("/p/prj_8f2ec0ffee00112233445566778899aa/chat/ch_00112233", "?thread=5"), { page: "chat", project: "prj_8f2ec0ffee00112233445566778899aa", room: "ch_00112233", thread: 5 });
  assert.equal(pathOf({ page: "chat", project: "prj_8f2ec0ffee00112233445566778899aa", room: "ch_00112233", thread: 5 }), "/p/prj_8f2ec0ffee00112233445566778899aa/chat/ch_00112233?thread=5");
  assert.equal(pathOf({ page: "chat", project: "prj_8f2ec0ffee00112233445566778899aa", view: "threads" }), "/p/prj_8f2ec0ffee00112233445566778899aa/chat/threads");
  assert.deepEqual(parse("/p/prj_8f2ec0ffee00112233445566778899aa/chat/threads"), { page: "chat", project: "prj_8f2ec0ffee00112233445566778899aa", view: "threads" });
  assert.equal(pathOf({ page: "dm", project: "prj_8f2ec0ffee00112233445566778899aa", room: "dm_00112233", msg: 2, thread: 1 }), "/p/prj_8f2ec0ffee00112233445566778899aa/dm/dm_00112233?msg=2&thread=1");
  assert.deepEqual(parse("/chat/threads"), { page: "find", what: "chat", view: "threads" });
  // Not a thread: left out.
  assert.deepEqual(parse("/p/prj_8f2ec0ffee00112233445566778899aa/chat/ch_00112233", "?thread=x"), { page: "chat", project: "prj_8f2ec0ffee00112233445566778899aa", room: "ch_00112233" });
});
