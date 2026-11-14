// The platform's links from the shell, inside the app (Arugula #989): which
// hrefs are the app's own places, followed in place. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { platformPath } from "../src/app/platform-here.ts";

const APP = "https://app.arugula.io";

test("a card, a board's row, an issue: the app's own places", () => {
  assert.equal(platformPath("/p/prj_1/issues/iss_1", APP), "/p/prj_1/issues/iss_1");
  assert.equal(platformPath("/p/prj_1/boards/brd_1#row=r_1", APP), "/p/prj_1/boards/brd_1#row=r_1");
  assert.equal(platformPath("/issues/iss_1?msg=3", APP), "/issues/iss_1?msg=3");
  assert.equal(platformPath(`${APP}/chat/room_1`, APP), "/chat/room_1");
});

test("anything else stays the browser's", () => {
  assert.equal(platformPath("https://github.com/acme/app/pull/7", APP), null, "another site");
  assert.equal(platformPath("https://control.arugula.io/p/prj_1", APP), null, "control's page");
  assert.equal(platformPath("/api/projects/prj_1/issues/export?format=md", APP), null, "the API");
  assert.equal(platformPath("#pane=3", `${APP}`), null, "this page's own hash");
});
