// Attention that belongs to no pane (#551, shared/notice): a mention that no
// session took is a card; a second in the same room counts up; they list
// oldest first. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { noticeOpenLabel, noticeRoom, noticeTitle, sortNotices } from "../src/shared/notice.ts";

const mention = (over = {}) => ({
  key: "mention:ch_x",
  kind: "mention",
  since_ms: 10,
  headline: "Ada mentioned this machine in #agents: are you there?",
  count: 1,
  room: "ch_x",
  room_name: "agents",
  ...over,
});

test("a mention alone says who, where and the first line; more count up", () => {
  assert.equal(noticeTitle(mention()), "Ada mentioned this machine in #agents: are you there?");
  assert.equal(noticeTitle(mention({ count: 2 })), "2 mentions in #agents");
  assert.equal(noticeTitle(mention({ count: 3, room_name: "#agents" })), "3 mentions in #agents");
  assert.equal(noticeTitle(mention({ count: 2, room_name: undefined })), "2 mentions in #ch_x");
});

test("opening a mention's card opens its room's chat", () => {
  assert.equal(noticeRoom(mention()), "ch_x");
  assert.equal(noticeRoom(mention({ room: undefined })), null);
  assert.equal(noticeOpenLabel(mention()), "Open chat");
});

test("cards list oldest first, and a new one goes to the end", () => {
  const a = { n: mention({ key: "mention:b", since_ms: 5 }) };
  const b = { n: mention({ key: "mention:a", since_ms: 9 }) };
  const c = { n: mention({ key: "mention:c", since_ms: 9 }) };
  assert.deepEqual(sortNotices([c, b, a]).map((i) => i.n.key), ["mention:b", "mention:a", "mention:c"]);
});
