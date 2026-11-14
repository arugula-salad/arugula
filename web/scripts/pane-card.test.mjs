// A pane's card chip (Arugula inversion): its words in the room a pane's
// bar has, the picker's choices and answers, and where a click goes. Run
// with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { cardChipText, cardChoices, cardNumber, cardPath, cardPicked, chipRoom } from "../src/shell/ui/pane-card-text.ts";

const TITLE = "L2: the platform knows which session is on a card";

test("the chip cuts the title first, then leaves only the card", () => {
  assert.deepEqual(cardChipText("Arugula", "#983", TITLE, 200), { project: "Arugula", card: "#983", title: TITLE });
  // "Arugula › #983 " is 15 characters: 30 leaves 15 of the title, its last an ellipsis.
  assert.deepEqual(cardChipText("Arugula", "#983", TITLE, 30), { project: "Arugula", card: "#983", title: "L2: the platfo…" });
  assert.deepEqual(cardChipText("Arugula", "#983", TITLE, 23), { project: "Arugula", card: "#983", title: "L2: the…" });
  assert.deepEqual(cardChipText("Arugula", "#983", TITLE, 22), { project: null, card: "#983", title: null });
  assert.deepEqual(cardChipText("Arugula", "#983", TITLE, 4), { project: null, card: "#983", title: null });
});

test("a card whose title isn't known yet shows the project and number while they fit", () => {
  assert.deepEqual(cardChipText("Arugula", "#983", null, 14), { project: "Arugula", card: "#983", title: null });
  assert.deepEqual(cardChipText("Arugula", "#983", "  ", 13), { project: null, card: "#983", title: null });
});

test("no card: the project and Put on a card, then the words alone", () => {
  assert.deepEqual(cardChipText("Arugula", null, null, 40), { project: "Arugula", card: null, title: null });
  assert.deepEqual(cardChipText("Arugula", null, null, 13), { project: null, card: null, title: null });
});

test("no chip when even the number doesn't fit, or Put on a card's words", () => {
  assert.equal(cardChipText("Arugula", "#983", TITLE, 3), null);
  assert.equal(cardChipText("Arugula", "#983", null, 0), null);
  assert.equal(cardChipText("Arugula", null, null, 12), null);
});

test("a pane's room: none in a narrow pane, so no chip there; more beside a short title", () => {
  // A quarter of a laptop's four columns: nothing fits beside the bar's own.
  assert.equal(chipRoom(130, 3), 0);
  assert.equal(cardChipText("Arugula", "#983", TITLE, chipRoom(130, 3)), null);
  assert.equal(cardChipText("Arugula", null, null, chipRoom(130, 3)), null);
  assert.equal(chipRoom(300, 3), 0);
  // Where #983 just fits, and its ▾ with it (the 50px the room leaves out).
  assert.equal(chipRoom(321, 3), 4);
  assert.deepEqual(cardChipText("Arugula", "#983", TITLE, chipRoom(321, 3)), { project: null, card: "#983", title: null });
  assert.equal(cardChipText("Arugula", "#983", TITLE, chipRoom(320, 3)), null);
  // A long title takes at most 40% of the bar.
  assert.equal(chipRoom(1000, 200), chipRoom(1000, 54));
  assert.ok(chipRoom(1000, 3) > chipRoom(1000, 40));
  assert.ok(chipRoom(1200, 3) > chipRoom(600, 3));
});

test("a card's number", () => {
  assert.equal(cardNumber("#983"), 983);
  assert.equal(cardNumber(" 12 "), 12);
  assert.equal(cardNumber("iss_0123abcd"), null);
});

const open = [
  { id: "iss_aa", number: 983, title: "L2: the platform knows" },
  { id: "iss_bb", number: 988, title: "L8: every pane points back" },
  { id: "iss_cc", number: 990, title: "L8 follow-up: the phone" },
];

test("the picker's choices put the pane's own card first", () => {
  const c = cardChoices(open, "#988");
  assert.deepEqual(c.map((x) => x.value), ["#988", "#983", "#990"]);
  assert.equal(c[0].strong, true);
  assert.equal(c[0].label, "#988 L8: every pane points back");
  assert.deepEqual(cardChoices(open, null).map((x) => x.value), ["#983", "#988", "#990"]);
});

test("an answer names a card by number, id or the one title that has its words", () => {
  assert.equal(cardPicked("#988", open), "#988");
  assert.equal(cardPicked("1234", open), "#1234");
  assert.equal(cardPicked("ISS_00ff", open), "iss_00ff");
  assert.equal(cardPicked("platform KNOWS", open), "#983");
  assert.equal(cardPicked("L8", open), null);
  assert.equal(cardPicked("nothing like it", open), null);
  assert.equal(cardPicked("  ", open), null);
});

test("a click goes to the card's board row, else its issue, else the project's issues", () => {
  assert.equal(cardPath("prj_1", { board: "brd_2", row: "row_3" }, "iss_aa"), "/p/prj_1/boards/brd_2#row=row_3");
  assert.equal(cardPath("prj_1", null, "iss_aa"), "/p/prj_1/issues/iss_aa");
  assert.equal(cardPath("prj_1", null, null), "/p/prj_1/issues");
});
