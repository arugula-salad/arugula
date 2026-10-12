// Where a tap on a notification goes (src/tap.ts, used by the service
// worker): the pane, the card, and a mention's thread (M61).

import { expect, test } from "@playwright/test";
import { shareUrl, tapUrl, withShare } from "../src/tap";

test("a tap opens the pane, and a mention's thread over it", () => {
  expect(tapUrl({ pane: 3 })).toBe("/#pane=3");
  expect(tapUrl({ pane: 3, thread: "pane-3" })).toBe("/#pane=3&thread=pane-3");
  expect(tapUrl({ pane: 3, thread: "session-2" })).toBe("/#pane=3&thread=session-2");
});

test("through control the pane is on its daemon", () => {
  expect(tapUrl({ pane: 3, daemon: "d1", thread: "pane-3" })).toBe("/#pane=d1.3&thread=pane-3");
  expect(tapUrl({ pane: 3, daemon: "d1" })).toBe("/#pane=d1.3");
});

test("a thread that isn't a pane's or a session's is dropped", () => {
  for (const thread of ["pane-x", "pane-3/../x", "other-3", "pane-3&x=1", "", "pane-"]) {
    expect(tapUrl({ pane: 3, thread })).toBe("/#pane=3");
  }
});

test("something that wants you opens at its card", () => {
  expect(tapUrl({ pane: 3, reason: { kind: "failed" } })).toBe("/#swarm=3");
  expect(tapUrl({ pane: 3, daemon: "d1", reason: { kind: "failed" } })).toBe("/#swarm=d1.3");
});

test("a push's Share opens the pane with the share picker (#237)", () => {
  expect(shareUrl({ pane: 3 })).toBe("/#pane=3&share");
  expect(shareUrl({ pane: 3, daemon: "d1" })).toBe("/#pane=d1.3&share");
});

test("Share comes first where there's room for it (#237)", () => {
  const dismiss = [{ action: "dismiss", title: "Dismiss" }];
  const reason = { kind: "done" };
  expect(withShare(dismiss, { pane: 3, reason }).map((a) => a.action)).toEqual(["share", "dismiss"]);
  expect(withShare([], { pane: 3, reason }).map((a) => a.action)).toEqual(["share"]);
  // Two already (Allow and Deny, a gate's, Rerun and Dismiss): no room.
  const two = [
    { action: "approve", title: "Allow" },
    { action: "deny", title: "Deny" },
  ];
  expect(withShare(two, { pane: 3, reason })).toEqual(two);
  // Not a pane that wants you: as it was.
  expect(withShare(dismiss, { pane: 3 })).toEqual(dismiss);
  expect(withShare(dismiss, { reason })).toEqual(dismiss);
});

test("with no pane it opens the app", () => {
  expect(tapUrl({})).toBe("/");
});
