// A board's words and paths (live status board, step 2): a row's glyph, a
// zone's progress, a pointer's label and the id a person types for one, and
// the board routes' paths. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { GLYPH, parsePointer, pointerHref, pointerLabel, progress, stepBefore } from "../src/shared/boards/text.ts";
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

test("a typed pointer is an id of a kind a row may point at", () => {
  assert.deepEqual(parsePointer(""), { kind: "none" });
  assert.deepEqual(parsePointer(" iss_0abc "), { kind: "issue", id: "iss_0abc" });
  assert.deepEqual(parsePointer("fi_0abc"), { kind: "forge_item", id: "fi_0abc" });
  assert.deepEqual(parsePointer("hw_0abc"), { kind: "workspace", id: "hw_0abc" });
  assert.equal(parsePointer("https://example.com/x"), null);
  assert.equal(parsePointer("brd_0abc"), null);
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
