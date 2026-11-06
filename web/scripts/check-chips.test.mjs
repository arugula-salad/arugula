import assert from "node:assert/strict";
import { test } from "node:test";
import { offenders } from "./check-chips.mjs";

test("a new chip, tag or badge class is refused outside the chip file", () => {
  assert.deepEqual(offenders(".run-chip { color: red; }", "platform/x.css").map((o) => o.name), ["run-chip"]);
  assert.deepEqual(offenders(".a .big-badge:hover,\n.x-tag {}", "shared/y.css").map((o) => [o.line, o.name]), [[1, "big-badge"], [2, "x-tag"]]);
});

test("the chip file, the shared tag, a comment and a container are fine", () => {
  assert.deepEqual(offenders(".foo-chip {}", "shared/ui/state-chip.css"), []);
  assert.deepEqual(offenders(".board .chip-tag { margin: 0; }", "shared/boards/boards.css"), []);
  assert.deepEqual(offenders("/* .old-tag was here */ .ws-tags {} .board-chips {}", "shared/style.css"), []);
});
