import assert from "node:assert/strict";
import { test } from "node:test";
import { offenders } from "./check-kit.mjs";

test("a new card, chip, badge, pill or row class in a .tsx is refused", () => {
  const src = '<div class="run-card">\n<span class={`x ${on ? "big-pill" : "new-row"}`} />\n<b className="a-badge" />';
  assert.deepEqual(
    offenders(src, "platform/x.tsx").map((o) => [o.line, o.name]),
    [
      [1, "run-card"],
      [2, "big-pill"],
      [2, "new-row"],
      [3, "a-badge"],
    ],
  );
});

test("the kit, the classes that predate it and a place class are fine", () => {
  assert.deepEqual(offenders('<div class="k-row is-card" />', "shared/kit/row.tsx"), []);
  assert.deepEqual(offenders('<div class="list-row" />', "shared/x.tsx"), []);
  assert.deepEqual(offenders('<div class="runs-rows rowing card-grid" />', "shared/x.tsx"), []);
});
