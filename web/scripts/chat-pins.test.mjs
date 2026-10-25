// A channel's topic and pins (#119): what a system line says, a pin's
// preview, and which messages can be pinned. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mayPin, preview, systemText } from "../src/shared/chat/pins.ts";

const sys = (kind, text = "", target = null) => ({ seq: 1, text, system: { kind, target }, deleted: null });

test("a topic line says the topic, or that it was cleared", () => {
  assert.deepEqual(systemText(sys("topic", "ship friday")), { before: "set the topic: ship friday", target: null, after: "" });
  assert.equal(systemText(sys("topic")).before, "cleared the topic");
  assert.equal(systemText(sys("description", "long text")).before, "changed the description");
});

test("a pin line names the message it is about, for a jump", () => {
  assert.deepEqual(systemText(sys("pin", "", 7)), { before: "pinned", target: 7, after: "" });
  assert.equal(systemText(sys("unpin", "", 7)).before, "unpinned");
});

test("a preview is one line, cut with an ellipsis", () => {
  assert.equal(preview("a\n\n  b\tc"), "a b c");
  const cut = preview("x".repeat(300), 20);
  assert.equal(cut.length, 20);
  assert.ok(cut.endsWith("…"));
});

test("said messages in a room you write in can be pinned; notes, deleted and archived ones not", () => {
  const room = { writable: true };
  const said = { imported: null, system: undefined, deleted: null };
  assert.equal(mayPin(room, said), true);
  assert.equal(mayPin({ writable: false }, said), false);
  assert.equal(mayPin(room, { ...said, deleted: 5 }), false);
  assert.equal(mayPin(room, { ...said, imported: "x" }), false);
  assert.equal(mayPin(room, { ...said, system: { kind: "pin", target: 1 } }), false);
});
