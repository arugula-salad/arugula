// #468: what the notification settings say of this device's notifications
// and of a test notification (src/shell/push-health.ts). Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { HOME_SCREEN, blockerOf, healthOf, standingSaid, testSaid } from "../src/shell/push-health.ts";

const standing = (over = {}) => ({ subscriptions: 2, usable: 2, this_device: "usable", ...over });

test("an iPhone in a Safari tab is told to add Arugula to its Home Screen", () => {
  assert.equal(HOME_SCREEN, "Add Arugula to your Home Screen to get notifications");
  assert.ok(blockerOf("install").startsWith(HOME_SCREEN));
  assert.match(blockerOf("denied"), /blocked/);
  assert.equal(blockerOf("on"), null);
  assert.equal(blockerOf("off"), null);
});

test("no subscription on this device, allowed or not, is said plainly", () => {
  assert.equal(healthOf("off", null, "granted"), "This device has no push subscription: turn on Notify this device.");
  assert.match(healthOf("off", null, "default"), /haven't been allowed here yet/);
  assert.match(healthOf("off", null, null), /haven't been allowed here yet/);
});

test("a subscription control lacks, or can't use for deploys, is said; a good one says nothing", () => {
  assert.equal(healthOf("on", standing(), "granted"), null);
  assert.equal(healthOf("on", null, "granted"), null, "not asked yet");
  assert.match(healthOf("on", standing({ this_device: "missing" }), "granted"), /doesn't have this device's subscription/);
  assert.match(healthOf("on", standing({ this_device: "unusable" }), "granted"), /deploy and project pushes skip it/);
  assert.equal(healthOf("unsupported", standing({ this_device: "missing" }), null), null, "the blocker says it");
});

test("the account's subscriptions, counted", () => {
  assert.equal(standingSaid(standing({ subscriptions: 0, usable: 0 })), "Your account has no push subscriptions.");
  assert.equal(standingSaid(standing({ subscriptions: 1, usable: 1 })), "Your account has 1 push subscription.");
  assert.equal(standingSaid(standing({ subscriptions: 3, usable: 2 })), "Your account has 3 push subscriptions; deploy pushes skip 1 of them (a device control no longer approves).");
});

test("a test notification says what became of it", () => {
  assert.match(testSaid(standing({ subscriptions: 0, usable: 0, sent: 0 })), /^Nothing to send to/);
  assert.equal(testSaid(standing({ sent: 2, gone: 0, failed: 0 })), "Sent a test to 2 of 2 subscriptions.");
  assert.equal(testSaid(standing({ subscriptions: 1, usable: 1, sent: 1 })), "Sent a test to 1 of 1 subscription.");
  assert.equal(
    testSaid(standing({ subscriptions: 4, usable: 3, sent: 2, gone: 1, failed: 1 })),
    "Sent a test to 2 of 4 subscriptions; 1 was gone and is forgotten; 1 failed. Deploy pushes skip 1: turn Notify this device off and on there.",
  );
});
