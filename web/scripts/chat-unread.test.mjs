// What counts as unread, and where (shared/chat/unread): the Chat button
// counts only the rooms chat lists, an object's thread counts on its
// issue's or run's row and its project's place, and the inbox's New in
// threads has each object thread with something unread that an unseen
// mention doesn't already point at. Run with `node
// --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { bold, counted, chatUnread, listedInChat, objectUnread, projectUnread, threadNews } from "../src/shared/chat/unread.ts";

const P = "prj_8f2ec0ffee00112233445566778899aa";
const Q = "prj_00112233445566778899aabbccddeeff";
let n = 0;
const room = (kind, over = {}) => ({
  id: `${kind.slice(0, 2)}_${String(++n).padStart(16, "0")}`,
  kind,
  team: kind === "channel" || kind === "dm" ? "t1" : null,
  name: kind,
  machine: null,
  machine_name: null,
  target: null,
  project: null,
  with: null,
  last: 5,
  read: 0,
  unread: 0,
  mentions: 0,
  muted: false,
  archived: false,
  writable: true,
  ...over,
});
const thread = (target, over = {}) => room("object", { target, project: P, name: target.startsWith("iss_") ? "issue #1" : "run 1a2b", ...over });
const entry = (item, reason, over = {}) => ({ item, reason, repo: "acme", number: 1, kind: "issue", state: "open", title: "", author: null, url: "", project: P, room: null, created_at: 0, updated_at: 0, seen_at: null, done_at: null, ...over });

test("chat lists channels, DMs, pane and session threads and archives, never an object's thread", () => {
  for (const k of ["channel", "dm", "pane", "session", "archive"]) assert.equal(listedInChat(room(k)), true, k);
  assert.equal(listedInChat(room("object", { target: "iss_1" })), false);
  // A channel or DM outside a team has no group to be in.
  assert.equal(listedInChat(room("channel", { team: null })), false);
});

test("the Chat button counts what chat lists, and only that", () => {
  const rooms = [
    room("channel", { unread: 2 }),
    room("dm", { unread: 1 }),
    room("pane", { unread: 3 }),
    // 93 posts by your own agents, in threads chat doesn't list (the bug).
    thread("iss_a", { unread: 60, mentions: 1 }),
    thread("run_b", { unread: 33 }),
  ];
  assert.deepEqual(chatUnread(rooms), { n: 6, mention: false });
  assert.deepEqual(chatUnread([thread("iss_a", { unread: 4, mentions: 4 })]), { n: 0, mention: false });
  // Muted: only its mentions count, and they still raise the mention.
  assert.deepEqual(chatUnread([room("channel", { unread: 9, mentions: 1, muted: true })]), { n: 1, mention: true });
});

test("an issue's or a run's row counts its own thread", () => {
  const rooms = [thread("iss_a", { unread: 3 }), thread("run_b", { unread: 2, mentions: 1 }), room("pane", { target: "iss_a", unread: 7 })];
  assert.deepEqual({ ...objectUnread(rooms, "iss_a"), room: undefined }, { n: 3, mention: false, room: undefined });
  assert.equal(objectUnread(rooms, "iss_a").room?.id, rooms[0].id);
  assert.equal(objectUnread(rooms, "run_b").mention, true);
  // Nobody's opened its thread: nothing.
  assert.deepEqual(objectUnread(rooms, "iss_none"), { n: 0, mention: false, room: undefined });
  assert.equal(objectUnread([thread("iss_m", { unread: 5, mentions: 0, muted: true })], "iss_m").n, 0);
});

test("a project's Issues and Runs places sum their threads", () => {
  const rooms = [
    thread("iss_a", { unread: 3 }),
    thread("iss_b", { unread: 2, mentions: 1 }),
    thread("run_c", { unread: 4 }),
    thread("iss_d", { unread: 9, project: Q }),
    thread("fi_e", { unread: 8 }),
    room("channel", { unread: 1 }),
  ];
  assert.deepEqual(projectUnread(rooms, P, "iss_"), { n: 5, mention: true });
  assert.deepEqual(projectUnread(rooms, P, "run_"), { n: 4, mention: false });
  assert.deepEqual(projectUnread(rooms, Q, "run_"), { n: 0, mention: false });
});

test("New in threads: object threads with something unread, unless an unseen mention points at it", () => {
  const issue = thread("iss_a", { unread: 2 });
  const run = thread("run_b", { unread: 5 });
  const mirrored = thread("fi_c", { unread: 1, name: "PR acme/app#3" });
  const mentionedIssue = thread("iss_d", { unread: 1, mentions: 1 });
  const read = thread("iss_e", { unread: 0 });
  const muted = thread("iss_f", { unread: 4, muted: true });
  const rooms = [issue, run, mirrored, mentionedIssue, read, muted, room("channel", { unread: 3 })];
  const ids = (list) => list.map((r) => r.target);

  // Mentions first, then the most unread.
  assert.deepEqual(ids(threadNews(rooms, [])), ["iss_d", "run_b", "iss_a", "fi_c"]);
  // An unseen mention already in the inbox: a native issue's by its item,
  // a mirrored item's thread_mention by its room.
  const entries = [entry("iss_d", "mentioned"), entry("fi_c", "thread_mention", { room: mirrored.id }), entry("iss_a", "assigned")];
  assert.deepEqual(ids(threadNews(rooms, entries)), ["run_b", "iss_a"]);
  // Once the mention's seen (or done), what's new after it is here again.
  const seen = [entry("iss_d", "mentioned", { seen_at: 1 }), entry("fi_c", "thread_mention", { room: mirrored.id, done_at: 1 })];
  assert.deepEqual(ids(threadNews(rooms, seen)), ["iss_d", "run_b", "iss_a", "fi_c"]);
  // Reading a thread (Done here, or opening it) takes it out.
  assert.deepEqual(ids(threadNews(rooms.map((r) => (r === run ? { ...r, unread: 0, read: r.last } : r)), entries)), ["iss_a"]);
});

test("a channel on mentions counts its mentions and shows its other unread as bold (#113)", () => {
  const quiet = room("channel", { level: "mentions", unread: 4, mentions: 0 });
  assert.equal(counted(quiet), 0);
  assert.equal(bold(quiet), true);
  assert.equal(counted({ ...quiet, mentions: 1 }), 1);
  // Every message counts them all; nothing counts only mentions and never bolds.
  assert.equal(counted({ ...quiet, level: "all" }), 4);
  const none = { ...quiet, level: "nothing", mentions: 2 };
  assert.deepEqual([counted(none), bold(none)], [2, true]);
  assert.deepEqual([counted({ ...none, mentions: 0 }), bold({ ...none, mentions: 0 })], [0, false]);
  // A DM, a pane's thread and an object's keep counting everything.
  for (const k of ["dm", "pane", "session"]) assert.equal(counted(room(k, { level: "mentions", unread: 3 })), 3, k);
  assert.equal(counted(thread("iss_x", { level: "mentions", unread: 2 })), 2);
  // The Chat button follows: a quiet channel adds nothing.
  assert.deepEqual(chatUnread([quiet, room("dm", { level: "all", unread: 1 })]), { n: 1, mention: false });
});
