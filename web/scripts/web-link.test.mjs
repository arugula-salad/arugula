// Links to open: web pages only. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { webLink } from "../src/shared/web-link.ts";

test("http and https links open", () => {
  assert.equal(webLink("https://example.com/x?y=1"), "https://example.com/x?y=1");
  assert.equal(webLink("HTTP://example.com/"), "HTTP://example.com/");
});

test("any other scheme, or no address at all, doesn't", () => {
  const others = ["javascript:alert(1)", " JaVaScRiPt:x", "file:///etc/passwd", "smb://h/s", "data:text/html,x", "mailto:a@b"];
  for (const url of [...others, "/relative", "", undefined, null]) {
    assert.equal(webLink(url), undefined, String(url));
  }
});
