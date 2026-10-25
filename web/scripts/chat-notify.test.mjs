// The snooze box's words and the presets (shared/chat/notify): a span or a
// clock time becomes the instant a snooze ends. Run with `node
// --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { clock, minutes, parseSnooze, snoozePresets } from "../src/shared/chat/notify-time.ts";

// A local Tuesday, 15:10.
const now = new Date(2026, 9, 27, 15, 10, 0).getTime();
const local = (d, h, m) => new Date(2026, 9, d, h, m, 0).getTime();

test("a span ends that long from now", () => {
  assert.equal(parseSnooze("45m", now), now + 45 * 60_000);
  assert.equal(parseSnooze("2 h", now), now + 2 * 3_600_000);
  assert.equal(parseSnooze("1.5h", now), now + 90 * 60_000);
});

test("a clock time is the next one: today, else tomorrow", () => {
  assert.equal(parseSnooze("17:30", now), local(27, 17, 30));
  assert.equal(parseSnooze("5pm", now), local(27, 17, 0));
  assert.equal(parseSnooze("9:15 am", now), local(28, 9, 15));
  assert.equal(parseSnooze("12am", now), local(28, 0, 0));
});

test("anything else is refused", () => {
  for (const t of ["", "soon", "25:00", "9:75", "-5m"]) assert.equal(parseSnooze(t, now), null, t);
});

test("the presets, and tomorrow is 8 in the morning", () => {
  const p = snoozePresets(now);
  assert.deepEqual(
    p.map((x) => x.label),
    ["30 minutes", "1 hour", "2 hours", "Until tomorrow"],
  );
  assert.equal(p[1].until, now + 3_600_000);
  assert.equal(p[3].until, local(28, 8, 0));
});

test("minutes of the day and back", () => {
  assert.equal(clock(22 * 60 + 5), "22:05");
  assert.equal(minutes("08:30"), 510);
  assert.equal(minutes(clock(1439)), 1439);
});
