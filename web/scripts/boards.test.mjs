// A board's words and paths (live status board, step 2): a row's glyph, a
// zone's progress, a pointer's label and the id a person types for one, and
// the board routes' paths. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { GLYPH, assigneesLine, debounced, isCard, parsePointer, pointerHref, pointerLabel, pointerToSend, progress, proven, stepActive, stepBefore, workLabel } from "../src/shared/boards/text.ts";
import { parse, pathOf } from "../src/platform/paths.ts";

const P = "prj_8f2ec0ffee00112233445566778899aa";
const B = "brd_0123456789abcdef0123456789abcdef";

test("a status is a glyph, and a zone's progress counts the done rows", () => {
  assert.deepEqual(Object.values(GLYPH), ["○", "○", "◐", "?", "●", "✕"]);
  const rows = ["done", "working", "done", "none"].map((status) => ({ status }));
  assert.deepEqual(progress({ rows }), { done: 2, total: 4, pct: 50 });
  assert.deepEqual(progress({ rows: [] }), { done: 0, total: 0, pct: 0 });
});

test("a pointer is labelled by its item when the platform read it", () => {
  const live = { state: "merged", kind: "pr", repo: "acme/app", number: 7, title: "Fix" };
  assert.equal(pointerLabel({ kind: "forge_item", id: "fi_abc123" }, live), "acme/app#7");
  assert.equal(pointerLabel({ kind: "forge_item", id: "fi_abc123" }), "PR or issue abc123");
  assert.equal(pointerLabel({ kind: "issue", id: "iss_abc123" }, { state: "open", kind: "native", number: 3, title: "x" }), "#3");
  assert.equal(pointerLabel({ kind: "block", machine: "m1", pane: "p2" }), "agent p2");
  assert.equal(pointerLabel({ kind: "none" }), "");
  const here = (path) => `https://app.example${path}`;
  assert.equal(pointerHref({ pointer: { kind: "forge_item", id: "fi_abc123" }, live }, here), "https://github.com/acme/app/pull/7");
  assert.equal(pointerHref({ pointer: { kind: "issue", id: "iss_abc123" } }, here), "https://app.example/issues/iss_abc123");
  assert.equal(pointerHref({ pointer: { kind: "block", machine: "m", pane: "p" } }, here), undefined);
});

test("an issue or a PR reads as a card, a run does not", () => {
  const live = (kind) => ({ state: "open", kind, number: 1, title: "x" });
  assert.deepEqual(["pr", "issue", "native", "run"].map((k) => isCard(live(k))), [true, true, true, false]);
  assert.equal(isCard(undefined), false);
  assert.equal(isCard(null), false);
});

test("a card's assignees are comma-separated, and shortened past three", () => {
  assert.equal(assigneesLine(undefined), "");
  assert.equal(assigneesLine([]), "");
  assert.equal(assigneesLine(["ana"]), "ana");
  assert.equal(assigneesLine(["ana", "bo", "cy"]), "ana, bo, cy");
  assert.equal(assigneesLine(["ana", "bo", "cy", "di"]), "ana, bo +2");
  assert.equal(assigneesLine(["a", "b", "c", "d", "e"]), "a, b +3");
});

test("a typed pointer is an id of a kind a row may point at", () => {
  assert.deepEqual(parsePointer(""), { kind: "none" });
  assert.deepEqual(parsePointer(" iss_0abc "), { kind: "issue", id: "iss_0abc" });
  assert.deepEqual(parsePointer("fi_0abc"), { kind: "forge_item", id: "fi_0abc" });
  assert.deepEqual(parsePointer("hw_0abc"), { kind: "workspace", id: "hw_0abc" });
  assert.deepEqual(parsePointer("https://example.com/x"), { kind: "link", url: "https://example.com/x" });
  assert.equal(parsePointer("http://example.com/x"), null);
  assert.equal(parsePointer("https://example.com/a b"), null);
  assert.equal(parsePointer("brd_0abc"), null);
});

test("a proofs row is proven by a link, a succeeded run or a merged PR", () => {
  const link = { pointer: { kind: "link", url: "https://ci.example.com/1" } };
  assert.equal(proven(link), true);
  assert.equal(pointerLabel(link.pointer), "ci.example.com/1");
  assert.equal(pointerHref(link, () => undefined), "https://ci.example.com/1");
  const live = (state) => ({ state, kind: "run", number: 0, title: "manual run" });
  assert.equal(proven({ pointer: { kind: "run", id: "run_a" }, live: live("succeeded") }), true);
  assert.equal(proven({ pointer: { kind: "run", id: "run_a" }, live: live("running") }), false);
  assert.equal(proven({ pointer: { kind: "run", id: "run_a" } }), false);
  assert.equal(proven({ pointer: { kind: "forge_item", id: "fi_a" }, live: live("merged") }), true);
  assert.equal(proven({ pointer: { kind: "none" } }), false);
});

test("a step moves an entry one place, or can't", () => {
  const ids = ["a", "b", "c"];
  assert.equal(stepBefore(ids, 0, true), null);
  assert.deepEqual(stepBefore(ids, 2, true), { before: "b" });
  assert.deepEqual(stepBefore(ids, 0, false), { before: "c" });
  assert.deepEqual(stepBefore(ids, 1, false), { before: null });
  assert.equal(stepBefore(ids, 2, false), null);
});

test("the board routes read and write the same paths", () => {
  assert.deepEqual(parse(`/p/${P}/boards`), { page: "boards", project: P });
  assert.deepEqual(parse(`/p/${P}/boards/${B}-launch`), { page: "board", project: P, board: B });
  assert.equal(parse(`/p/${P}/boards/iss_0123`).page, "missing");
  const names = (id) => ({ [P]: "Arugula", [B]: "Launch" })[id];
  assert.equal(pathOf({ page: "board", project: P, board: B }, names), `/p/${P}-arugula/boards/${B}-launch`);
  assert.equal(pathOf({ page: "boards", project: P }), `/p/${P}/boards`);
});

test("a result is labelled by its source", () => {
  assert.equal(workLabel({ source: "github", repo: "acme/app", number: 7, title: "Fix" }), "acme/app#7");
  assert.equal(workLabel({ source: "native", repo: null, number: 3, title: "Fix" }), "#3");
  assert.equal(workLabel({ source: "run", repo: null, number: null, title: "manual run" }), "manual run");
});

test("the arrow keys step through the results and wrap", () => {
  assert.equal(stepActive(-1, 3, true), 0);
  assert.equal(stepActive(2, 3, true), 0);
  assert.equal(stepActive(-1, 3, false), 2);
  assert.equal(stepActive(0, 3, false), 2);
  assert.equal(stepActive(1, 3, false), 0);
  assert.equal(stepActive(0, 0, true), -1);
});

test("a search waits for the last keystroke", async () => {
  const seen = [];
  const f = debounced((q) => seen.push(q), 20);
  f("a");
  f("ab");
  await new Promise((r) => setTimeout(r, 60));
  assert.deepEqual(seen, ["ab"]);
  f("c");
  f.cancel();
  await new Promise((r) => setTimeout(r, 60));
  assert.deepEqual(seen, ["ab"]);
});

test("a row sends a pointer that was picked or typed, never one it only kept", () => {
  const picked = { kind: "issue", id: "iss_0abc" };
  // Picked, then the note edited: the picked pointer still goes.
  assert.deepEqual(pointerToSend("#3", "", picked), { pointer: picked });
  assert.deepEqual(pointerToSend("#3", "https://ci.example.com/1", picked), { pointer: picked });
  // A note edited alone: no pointer, so a link or a block's stays.
  assert.deepEqual(pointerToSend("https://ci.example.com/1", "https://ci.example.com/1", null), {});
  assert.deepEqual(pointerToSend("", "", null), {});
  // A typed id or link goes as before; text that is neither is refused.
  assert.deepEqual(pointerToSend("iss_0abc", "", null), { pointer: picked });
  assert.deepEqual(pointerToSend("https://example.com/x", "", null), { pointer: { kind: "link", url: "https://example.com/x" } });
  assert.equal(pointerToSend("some words", "", null), null);
});
