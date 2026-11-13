// A Doc's title (#979): the same limits the platform and the daemon's title_ok
// hold, checked before anything is sent. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { TITLE_MAX, titleRefusal } from "../src/shared/docs/doc-title.ts";

test("a title is trimmed, not empty, one line and at most 200 characters", () => {
  assert.equal(titleRefusal("Release plan"), null);
  assert.equal(titleRefusal("  padded  "), null);
  assert.equal(titleRefusal("x".repeat(TITLE_MAX)), null);
  assert.equal(titleRefusal("é".repeat(TITLE_MAX)), null, "characters, not bytes");
  assert.match(titleRefusal("   "), /needs a title/);
  assert.match(titleRefusal(""), /needs a title/);
  assert.match(titleRefusal("one\ntwo"), /one line/);
  assert.match(titleRefusal("one\r\ntwo"), /one line/);
  assert.match(titleRefusal("x".repeat(TITLE_MAX + 1)), /over 200/);
  assert.equal(titleRefusal("trailing\n"), null, "trimmed first");
});
