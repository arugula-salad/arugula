// The key table's dispatch (Arugula #1535): one key runs one binding, the
// most specific that takes it (a dialog's, a screen's, the panel's, then
// one that's everywhere; of two as specific, the newer), and a pending `g`
// lets go once focus is in a field. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";

// The table listens on the window: a bare one does here.
globalThis.window = new EventTarget();
const { addBindings, both, focusMoved, press } = await import("../src/shared/keys.ts");

let clock = 0;
const key = (k, extra = {}) => ({
  key: k,
  code: /^[a-z]$/.test(k) ? `Key${k.toUpperCase()}` : k,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  shiftKey: false,
  isComposing: false,
  timeStamp: (clock += 10),
  ...extra,
});

/** Bindings that note what ran, added for one test. */
function table(list) {
  const ran = [];
  const off = addBindings(list.map((b) => ({ label: b.id, keys: both(b.key), ...b, run: () => ran.push(b.id) })));
  return { ran, off };
}

test("a screen's key beats the panel's, which beats one that's everywhere: one runs", () => {
  const { ran, off } = table([
    { id: "board.lead", key: "key:l" },
    { id: "panel-focus", key: "key:l", context: "panel" },
    { id: "panel-floor", key: "key:f", context: "panel" },
    { id: "board.find", key: "key:f" },
    { id: "look", key: "key:s", context: "everywhere" },
    { id: "panel-collapse", key: "key:s", context: "panel" },
  ]);
  assert.equal(press(key("l"), false), true);
  assert.equal(press(key("f"), false), true);
  assert.equal(press(key("s"), false), true);
  assert.deepEqual(ran, ["board.lead", "board.find", "panel-collapse"]);
  off();
});

test("a dialog's Escape beats the screen's, and of two dialogs the newer closes", () => {
  const { ran, off } = table([
    { id: "screen.back", key: "key:escape" },
    { id: "dialog.under", key: "*+key:escape", context: "dialog" },
    { id: "dialog.over", key: "*+key:escape", context: "dialog" },
  ]);
  press(key("Escape", { code: "Escape" }), false);
  assert.deepEqual(ran, ["dialog.over"]);
  off();
});

test("a binding whose `when` doesn't hold lets the next one have the key", () => {
  const { ran, off } = table([
    { id: "goto", key: "Meta+key:k", context: "everywhere" },
    { id: "chat.rooms", key: "Meta+key:k", when: () => false },
  ]);
  press(key("k", { metaKey: true }), false);
  assert.deepEqual(ran, ["goto"]);
  off();
  const chat = table([
    { id: "goto", key: "Meta+key:k", context: "everywhere" },
    { id: "chat.rooms", key: "Meta+key:k" },
  ]);
  press(key("k", { metaKey: true }), false);
  assert.deepEqual(chat.ran, ["chat.rooms"]);
  chat.off();
});

test("a key no binding takes goes on", () => {
  const { ran, off } = table([{ id: "j", key: "key:j" }]);
  assert.equal(press(key("q"), false), false);
  assert.equal(press(key("j"), true), false, "a plain key is never taken from a field");
  assert.deepEqual(ran, []);
  off();
});

test("`g` then a letter runs the one sequence; the most specific of two", () => {
  const { ran, off } = table([
    { id: "go.overview", key: "key:g key:o", context: "everywhere" },
    { id: "chat.as-pane", key: "key:o" },
    { id: "go.agents", key: "key:g key:a", context: "everywhere" },
    { id: "board.agents-pane", key: "key:g key:a" },
  ]);
  assert.equal(press(key("g"), false), true);
  assert.equal(press(key("o"), false), true);
  press(key("g"), false);
  press(key("a"), false);
  press(key("o"), false);
  assert.deepEqual(ran, ["go.overview", "board.agents-pane", "chat.as-pane"]);
  off();
});

test("a pending `g` lets go when focus moves into a field: the next key is typed", () => {
  const { ran, off } = table([{ id: "go.overview", key: "key:g key:o", context: "everywhere" }]);
  press(key("g"), false);
  focusMoved(false);
  assert.equal(press(key("o"), false), true, "focus on a button keeps the sequence");
  press(key("g"), false);
  focusMoved(true);
  assert.equal(press(key("o"), true), false, "the o reaches the field");
  assert.equal(press(key("x"), false), false, "and nothing is pending after");
  press(key("g"), false);
  assert.equal(press(key("o", { ctrlKey: true }), true), false, "a chord in a field after g isn't eaten");
  assert.deepEqual(ran, ["go.overview"]);
  off();
});

test("a stray `g` outside a field still waits 1.5 s, then lets go", () => {
  const { ran, off } = table([{ id: "go.overview", key: "key:g key:o", context: "everywhere" }]);
  press(key("g"), false);
  assert.equal(press(key("q"), false), true, "the key after g is the sequence's");
  press(key("g"), false);
  clock += 2000;
  assert.equal(press(key("o"), false), false);
  assert.deepEqual(ran, []);
  off();
});
