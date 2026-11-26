import assert from "node:assert/strict";
import { test } from "node:test";
import { lowered, over } from "./check-budget.mjs";
import { measure } from "./measure.mjs";

const budget = { max: { cssLines: 100, looks: 216 }, report: ["tsLines"] };

test("a number past its ceiling fails, with how far", () => {
  assert.deepEqual(over(budget, { cssLines: 101, looks: 216, tsLines: 9 }), ["cssLines: 101 > 100 (+1)"]);
});

test("at or under every ceiling passes, and report-only numbers never fail", () => {
  assert.deepEqual(over(budget, { cssLines: 100, looks: 2, tsLines: 1e9 }), []);
});

test("--write only ever lowers a ceiling", () => {
  assert.deepEqual(lowered(budget, { cssLines: 90, looks: 300 }).max, { cssLines: 90, looks: 216 });
  assert.deepEqual(lowered(budget, {}).report, ["tsLines"]);
});

test("measure prints every number the budget names", () => {
  const now = measure();
  for (const key of Object.keys(budget.max).concat(budget.report)) assert.equal(typeof now[key], "number", key);
});
