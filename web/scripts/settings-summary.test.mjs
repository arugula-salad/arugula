// What Settings' list says of a section (Arugula #1104): the shared words,
// and quiet hours in a line. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { joined, plural, quietSummary, some, span, spoken } from "../src/platform/settings/summary.ts";

test("counts and names, in a few words", () => {
  assert.equal(plural(1, "repo"), "1 repo");
  assert.equal(plural(3, "repo"), "3 repos");
  assert.equal(plural(2, "secret key", "secret keys"), "2 secret keys");
  assert.equal(some(["deploy"]), "deploy");
  assert.equal(some(["deploy", "outage"]), "deploy, outage");
  assert.equal(some(["deploy", "outage", "p0", "sev1"]), "deploy, outage +2");
  assert.equal(joined("Team plan", false, null, "", "6 seats"), "Team plan · 6 seats");
});

test("a span of minutes in words", () => {
  assert.equal(span(45), "45 min");
  assert.equal(span(90), "1h 30m");
  assert.equal(span(120), "2 hours");
  assert.equal(span(1440), "1 day");
  assert.equal(span(2880), "2 days");
});

test("a time as people say it", () => {
  assert.equal(spoken(0), "12 AM");
  assert.equal(spoken(22 * 60), "10 PM");
  assert.equal(spoken(7 * 60 + 30), "7:30 AM");
  assert.equal(spoken(12 * 60), "12 PM");
});

test("quiet hours: the nights, one window and the zone's city; off when none", () => {
  assert.equal(quietSummary([], "UTC"), "Off");
  assert.equal(quietSummary([{ days: [1, 2, 3, 4, 5], from: 22 * 60, to: 8 * 60 }], "America/New_York"), "Weeknights 10 PM – 8 AM · New York");
  assert.equal(quietSummary([{ days: [1, 2, 3, 4, 5, 6, 7], from: 23 * 60, to: 7 * 60 }], "UTC"), "Every night 11 PM – 7 AM · UTC");
  assert.equal(quietSummary([{ days: [6, 7], from: 0, to: 9 * 60 }], "Europe/London"), "Weekends 12 AM – 9 AM · London");
  assert.equal(quietSummary([{ days: [3, 1], from: 21 * 60, to: 6 * 60 }], "UTC"), "Mon, Wed 9 PM – 6 AM · UTC");
  // Two windows: the nights, and that the times vary.
  assert.equal(
    quietSummary(
      [
        { days: [1, 2, 3, 4], from: 22 * 60, to: 8 * 60 },
        { days: [5], from: 23 * 60, to: 9 * 60 },
      ],
      "UTC",
    ),
    "Weeknights, times vary · UTC",
  );
});
