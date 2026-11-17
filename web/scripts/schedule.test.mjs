// Weekdays and TimeRange's values (Arugula #1101): a day toggled stays once
// and in week order, a range that ends before it starts runs overnight, and
// a day picked in one window leaves the others. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { claimDays, clockMinutes, overnight, toggleDay, WEEKDAYS } from "../src/shared/ui/days.ts";

test("seven days, Monday first", () => {
  assert.deepEqual(
    WEEKDAYS.map((d) => d.day),
    [1, 2, 3, 4, 5, 6, 7],
  );
  assert.equal(WEEKDAYS.map((d) => d.letter).join(""), "MTWTFSS");
});

test("a day toggles on and off, once, in week order", () => {
  assert.deepEqual(toggleDay([], 3), [3]);
  assert.deepEqual(toggleDay([5, 1], 3), [1, 3, 5]);
  assert.deepEqual(toggleDay([1, 3, 5], 3), [1, 5]);
  assert.deepEqual(toggleDay([7], 7), []);
  assert.deepEqual(toggleDay([2], 9), [2]);
});

test("a time is minutes past midnight", () => {
  assert.equal(clockMinutes("00:00"), 0);
  assert.equal(clockMinutes("22:30"), 22 * 60 + 30);
  assert.ok(Number.isNaN(clockMinutes("")));
  assert.ok(Number.isNaN(clockMinutes("24:00")));
});

test("a range that ends at or before it starts runs overnight", () => {
  assert.equal(overnight("22:00", "08:00"), true);
  assert.equal(overnight("09:00", "09:00"), true);
  assert.equal(overnight("09:00", "17:00"), false);
  assert.equal(overnight("", "08:00"), false);
});

test("a day picked in one window leaves the others", () => {
  const wins = [
    { days: [1, 2, 3, 4, 5], from: "22:00", to: "08:00" },
    { days: [6, 7], from: "00:00", to: "10:00" },
  ];
  assert.deepEqual(
    claimDays(wins, 1, [5, 6, 7]).map((w) => w.days),
    [
      [1, 2, 3, 4],
      [5, 6, 7],
    ],
  );
  // A later window left with no days goes; the first stays.
  assert.deepEqual(
    claimDays(wins, 0, [1, 2, 3, 4, 5, 6, 7]).map((w) => w.days),
    [[1, 2, 3, 4, 5, 6, 7]],
  );
  assert.deepEqual(
    claimDays(wins, 1, [1, 2, 3, 4, 5, 6, 7]).map((w) => w.days),
    [[], [1, 2, 3, 4, 5, 6, 7]],
  );
  // The times go with their window.
  assert.equal(claimDays(wins, 1, [5, 6, 7])[1].from, "00:00");
});
