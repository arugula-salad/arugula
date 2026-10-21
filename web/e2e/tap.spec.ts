// Where a tap on a notification goes (src/tap.ts, used by the service
// worker): the pane, the card, and a room of control's chat (M123), on
// the platform page once control has one (M156).

import { expect, test } from "@playwright/test";
import { chatTap, platformOf, platformTap, shareUrl, tapUrl, withShare } from "../src/shell/tap";

test("a chat push from control opens its room at the message (M123)", () => {
  const room = `ch_${"0a".repeat(8)}`;
  expect(chatTap({ chat: { room, seq: 7 } })).toBe(`/#chat=${room}&msg=7`);
  expect(chatTap({ chat: { room: `dm_${"1f".repeat(8)}` } })).toBe(`/#chat=dm_${"1f".repeat(8)}`);
  // Imported messages are numbered 0 and below.
  expect(chatTap({ chat: { room, seq: -3 } })).toBe(`/#chat=${room}&msg=-3`);
  // Not a room's id, or no room: nothing to open.
  for (const bad of ["ch_", "CH_0a0a0a0a", `ch_${"0a".repeat(8)}/../x`, `ch_${"0a".repeat(8)}&x=1`, 3, null]) {
    expect(chatTap({ chat: { room: bad, seq: 1 } })).toBeUndefined();
  }
  expect(chatTap({})).toBeUndefined();
});

test("with a platform page, a chat push or a Cloud tab's opens there, by id (M156)", () => {
  const room = `ch_${"0a".repeat(8)}`;
  expect(platformTap({ chat: { room, seq: 7 } })).toBe(`/chat/${room}?msg=7`);
  expect(platformTap({ chat: { room: `dm_${"1f".repeat(8)}` } })).toBe(`/chat/dm_${"1f".repeat(8)}`);
  expect(platformTap({ cloud_run: "run_00ff11ee" })).toBe("/past/run_00ff11ee");
  expect(platformTap({ cloud_workspace: "cw_0123abcd", cloud_tab: "cwc_4567ef01" })).toBe("/workspaces/cw_0123abcd/cwc_4567ef01");
  // M171: an inbox push opens the inbox.
  expect(platformTap({ inbox: { item: `fi_${"0a".repeat(16)}` } })).toBe("/inbox");
  // Anything else stays the shell's.
  for (const n of [{}, { chat: { room: "ch_" } }, { cloud_run: "run_x" }, { cloud_workspace: "cw_0123abcd" }, { pane: 3 }]) expect(platformTap(n)).toBeUndefined();
  expect(platformOf({ platform: "https://app.arugula.io" })).toBe("https://app.arugula.io");
  for (const bad of [{}, null, { platform: null }, { platform: "https://app.arugula.io/x" }, { platform: "javascript:alert(1)" }]) expect(platformOf(bad)).toBeNull();
});

test("a tap opens the pane", () => {
  expect(tapUrl({ pane: 3 })).toBe("/#pane=3");
});

test("through control the pane is on its daemon", () => {
  expect(tapUrl({ pane: 3, daemon: "d1" })).toBe("/#pane=d1.3");
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
