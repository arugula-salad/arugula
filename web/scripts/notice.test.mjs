// Attention that belongs to no pane (#551, shared/notice): a mention that no
// session took is a card; a second in the same room counts up; they list
// oldest first. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { grantQuestion, noticeOpenLabel, noticeRoom, noticeTitle, sortNotices } from "../src/shared/notice.ts";

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

const ask = (over = {}, grant = {}) => ({
  key: "grant:ch_x",
  kind: "grant_ask",
  since_ms: 10,
  headline: "Ada mentioned geek in #agents.",
  count: 1,
  room: "ch_x",
  room_name: "agents",
  grant: { machine: "m_geek", machine_name: "geek", scope_kind: "project", scope_id: "p1", scope_name: "Acme", ...grant },
  ...over,
});

test("a grant card says what Grant reaches: the project's channels and issues (#578)", () => {
  assert.equal(
    noticeTitle(ask()),
    "Ada mentioned geek in #agents. Let geek's agents read Acme's channels and issues (including #agents)?",
  );
  assert.equal(
    noticeTitle(ask({ count: 2 })),
    "2 mentions of geek in #agents. Let geek's agents read Acme's channels and issues (including #agents)?",
  );
  assert.equal(noticeRoom(ask()), null);
});

test("a grant over just the room says only the room; over DMs, all of them", () => {
  assert.equal(noticeTitle(ask({}, { scope_name: "" })), "Ada mentioned geek in #agents. Let geek's agents read #agents?");
  assert.equal(grantQuestion(ask({ room_name: "#agents" }, { scope_name: "" })), "Let geek's agents read #agents?");
  assert.equal(
    grantQuestion(ask({}, { scope_kind: "account", scope_name: "your DMs" })),
    "Let geek's agents read all your DMs (including #agents)?",
  );
});
