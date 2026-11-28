// The Saved and Threads badges count what their views show (Arugula #1529).
// Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { savedIn, threadsIn } from "../src/shared/chat/badges.ts";

const room = (id, kind, team) => ({ id, kind, team });
const item = (r, seq, done = null) => ({ room: r, seq, done });
const rooms = [room("ch1", "channel", "t1"), room("dm1", "dm", "t1"), room("ch2", "channel", "t2"), room("pane", "pane", null)];
const find = (id) => rooms.find((r) => r.id === id);
const saved = [item("ch1", 1), item("ch1", 2, 5), item("dm1", 1), item("ch2", 1), item("pane", 1), item("gone", 1)];
const todo = (scope) => savedIn(saved, find, scope).filter((i) => i.done === null).length;
const totals = { n: 10, mentions: 3 };
const project = { team: "t1", name: "P" };
const personal = { team: null, name: "Me" };

test("Saved counts the to-do items of the scope's rooms, no DMs", () => {
  assert.equal(todo(project), 1);
  assert.equal(todo(personal), 1);
  assert.equal(todo(), 5);
  assert.deepEqual(savedIn(saved, find, project).map((i) => i.seq), [1, 2]);
});

test("Threads sums the rooms its view shows", () => {
  const by = [
    { room: "ch1", unread: 2, mentions: 1 },
    { room: "dm1", unread: 3, mentions: 1 },
    { room: "ch2", unread: 4, mentions: 0 },
    { room: "pane", unread: 5, mentions: 2 },
    { room: "gone", unread: 1, mentions: 0 },
  ];
  assert.deepEqual(threadsIn(by, totals, find, project), { n: 2, mentions: 1 });
  assert.deepEqual(threadsIn(by, totals, find, personal), { n: 5, mentions: 2 });
  assert.deepEqual(threadsIn(by, totals, find), { n: 15, mentions: 4 });
});

test("Threads falls back to the totals from a server that sends no rooms", () => {
  assert.deepEqual(threadsIn(null, totals, find, project), { n: 10, mentions: 3 });
  assert.deepEqual(threadsIn(null, totals, find), { n: 10, mentions: 3 });
});
