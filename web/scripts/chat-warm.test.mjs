// Warming the box (M262), the page's part: which text asks `@arugula`,
// which rooms warm, and one warm ping per composing burst. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { asksArugula, burstOf, typingBody, warmsIn, Warming } from "../src/shared/chat/warm.ts";

test("text asks @arugula when it starts with it or has it as a word", () => {
  assert.ok(asksArugula("@arugula fix the canary"));
  assert.ok(asksArugula("@arug") === false);
  assert.ok(asksArugula("@arugula"), "before the word is done");
  assert.ok(asksArugula("  @Arugula"));
  assert.ok(asksArugula("hey @arugula please look"));
  assert.ok(asksArugula("(@arugula look"));
  assert.ok(!asksArugula("hey @arugula"), "anywhere else, once a space follows");
  assert.ok(!asksArugula("mail me@arugula.io later"));
  assert.ok(!asksArugula("ask arugula about it"));
});

test("only a project's channels and object threads warm", () => {
  assert.ok(warmsIn("channel"));
  assert.ok(warmsIn("object"));
  for (const k of ["dm", "pane", "session", "archive"]) assert.ok(!warmsIn(k), k);
});

test("one warm ping per burst, per room", () => {
  const w = new Warming();
  assert.equal(w.ping("ch_1", false), false);
  assert.equal(w.ping("ch_1", true), true);
  assert.equal(w.ping("ch_1", true), false, "the same burst");
  assert.equal(w.ping("ch_2", true), true, "another room's own");
  w.ended("ch_1");
  assert.equal(w.ping("ch_1", true), true, "the next message");
});

test("a reply thread's composer sends only a warm ping, never typing", () => {
  assert.equal(typingBody(false, false), undefined, "the room's: plain typing");
  assert.deepEqual(typingBody(true, false), { warm: true, show: true });
  assert.deepEqual(typingBody(true, true), { warm: true, show: false }, "warm-only");
  assert.equal(typingBody(false, true), null, "nothing at all");
  const w = new Warming();
  assert.equal(w.ping(burstOf("ch_1"), true), true);
  assert.equal(w.ping(burstOf("ch_1", 5), true), true, "a thread's burst is its own");
  assert.equal(w.ping(burstOf("ch_1", 5), true), false);
});
