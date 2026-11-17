// Weekdays and TimeRange's values (Arugula #1101): a day toggled stays once
// and in week order, a range that ends before it starts runs overnight, and
// a day picked in one window leaves the others; the week they make, drawn
// (Arugula #1109), runs an overnight window into the next morning. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { claimDays, clockMinutes, clockOf, overnight, sameTimes, toggleDay, weekSpans, WEEKDAYS } from "../src/shared/ui/days.ts";

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

test("the week: a window on its days, an overnight one into the next morning", () => {
  const h = (n) => n * 60;
  // Weeknights 10 PM to 8 AM: Monday ends quiet, Saturday starts it, Sunday's free.
  const week = weekSpans([{ days: [1, 2, 3, 4, 5], from: "22:00", to: "08:00" }]);
  assert.deepEqual(week[0], [{ from: h(22), to: h(24) }]);
  assert.deepEqual(week[1], [
    { from: 0, to: h(8) },
    { from: h(22), to: h(24) },
  ]);
  assert.deepEqual(week[5], [{ from: 0, to: h(8) }]);
  assert.deepEqual(week[6], []);
  // Sunday's night runs into Monday.
  assert.deepEqual(weekSpans([{ days: [7], from: "23:00", to: "07:00" }])[0], [{ from: 0, to: h(7) }]);
  // A daytime window stays on its day.
  assert.deepEqual(weekSpans([{ days: [3], from: "09:00", to: "17:30" }])[2], [{ from: h(9), to: h(17) + 30 }]);
  // Ending at midnight takes nothing from the next day.
  assert.deepEqual(weekSpans([{ days: [1], from: "20:00", to: "00:00" }])[1], []);
});

test("the week: windows that meet join, one that ends where it starts is the whole day round", () => {
  const h = (n) => n * 60;
  const week = weekSpans([
    { days: [1], from: "22:00", to: "06:00" },
    { days: [2], from: "05:00", to: "09:00" },
  ]);
  assert.deepEqual(week[1], [{ from: 0, to: h(9) }]);
  assert.deepEqual(weekSpans([{ days: [4], from: "00:00", to: "00:00" }])[3], [{ from: 0, to: h(24) }]);
  const round = weekSpans([{ days: [4], from: "21:00", to: "21:00" }]);
  assert.deepEqual(round[3], [{ from: h(21), to: h(24) }]);
  assert.deepEqual(round[4], [{ from: 0, to: h(21) }]);
});

test("the week: a time mid-edit draws nothing, nor does a day that isn't one", () => {
  assert.deepEqual(weekSpans([{ days: [1, 2], from: "", to: "08:00" }]).flat(), []);
  assert.deepEqual(weekSpans([{ days: [0, 8], from: "22:00", to: "08:00" }]).flat(), []);
});

test("minutes as a clock reads them", () => {
  assert.equal(clockOf(0), "00:00");
  assert.equal(clockOf(8 * 60 + 5), "08:05");
  assert.equal(clockOf(24 * 60), "24:00");
});

test("same times every night: every day the windows cover, at the first one's times", () => {
  const wins = [
    { days: [1, 2, 3, 4], from: "22:00", to: "07:00" },
    { days: [7, 5], from: "23:30", to: "10:00" },
  ];
  assert.deepEqual(sameTimes(wins), [{ days: [1, 2, 3, 4, 5, 7], from: "22:00", to: "07:00" }]);
  assert.deepEqual(sameTimes([]), []);
});
