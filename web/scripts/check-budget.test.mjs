import assert from "node:assert/strict";
import { test } from "node:test";
import { lowered, over } from "./check-budget.mjs";
import { measure } from "./measure.mjs";

const budget = { max: { cssLines: 110, looks: 216 }, headroom: { cssLines: 10, looks: 0 }, report: ["tsLines"] };

test("a number past its ceiling fails, with how far", () => {
  assert.deepEqual(over(budget, { cssLines: 111, looks: 216, tsLines: 9 }), ["cssLines: 111 > 110 (+1)"]);
});

test("at or under every ceiling passes, headroom included, and report-only numbers never fail", () => {
  assert.deepEqual(over(budget, { cssLines: 110, looks: 2, tsLines: 1e9 }), []);
});

test("--write lowers a ceiling to now plus its headroom, and never raises one", () => {
  assert.deepEqual(lowered(budget, { cssLines: 90, looks: 2 }).max, { cssLines: 100, looks: 2 });
  assert.deepEqual(lowered(budget, { cssLines: 105, looks: 300 }).max, { cssLines: 110, looks: 216 });
  assert.deepEqual(lowered(budget, {}).max, budget.max);
  assert.deepEqual(lowered(budget, {}).report, ["tsLines"]);
});

test("measure prints every number the budget names", () => {
  const now = measure();
  for (const key of Object.keys(budget.max).concat(budget.report)) assert.equal(typeof now[key], "number", key);
});
