// Dates and times, one way on both surfaces (UI audit #182): how long ago
// for a week, then the day; a time to come; the words in a sentence. Run
// with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { ago, day } from "../src/shared/time.ts";

const now = new Date(2026, 9, 26, 12).getTime();
const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

test("how long ago, for a week", () => {
  assert.equal(ago(now - 10_000, now), "just now");
  assert.equal(ago(now - 3 * MIN, now), "3m ago");
  assert.equal(ago(now - 5 * HOUR, now), "5h ago");
  assert.equal(ago(now - 2 * DAY, now), "2d ago");
  assert.equal(ago(now - 6 * DAY, now), "6d ago");
});

test("then the day, with the year only when it isn't this one", () => {
  assert.equal(ago(now - 8 * DAY, now), day(now - 8 * DAY, now));
  assert.doesNotMatch(day(now - 8 * DAY, now), /2026/);
  assert.match(day(new Date(2025, 11, 30).getTime(), now), /2025/);
  assert.equal(ago(now - 8 * DAY, now, "on"), `on ${day(now - 8 * DAY, now)}`);
});

test("a time to come; a clock a little ahead is now", () => {
  assert.equal(ago(now + 7 * HOUR + 5 * MIN, now), "in 7h");
  assert.equal(ago(now + 3 * DAY, now), "in 3d");
  assert.equal(ago(now + 30_000, now), "just now");
  assert.equal(ago(now + 30 * DAY, now), day(now + 30 * DAY, now));
});

test("in a sentence, units spelled out", () => {
  assert.equal(ago(now - MIN, now, "long"), "1 minute ago");
  assert.equal(ago(now - 3 * DAY, now, "long"), "3 days ago");
  assert.equal(ago(now + 2 * HOUR, now, "long"), "in 2 hours");
  assert.match(ago(now - 90 * DAY, now, "long"), /^on /);
});
