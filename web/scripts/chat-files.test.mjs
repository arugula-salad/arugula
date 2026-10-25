// Chat's files (#107): the words for a file's size and kind, and the name a
// pasted picture gets. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { kindText, MAX_FILE_BYTES, pastedName, sizeText, tooBig } from "../src/shared/chat/files.ts";

test("sizes read short", () => {
  assert.equal(sizeText(37), "37 B");
  assert.equal(sizeText(2048), "2 KB");
  assert.equal(sizeText(1.4 * 1024 * 1024), "1.4 MB");
  assert.equal(sizeText(25 * 1024 * 1024), "25 MB");
});

test("a card says what a file is by its extension, else its type", () => {
  assert.equal(kindText({ name: "plan.pdf", mime: "application/pdf" }), "PDF");
  assert.equal(kindText({ name: "Makefile", mime: "text/plain" }), "PLAIN");
  assert.equal(kindText({ name: "blob", mime: "application/octet-stream" }), "FILE");
});

test("a pasted picture is named for when it was pasted; a real name stays", () => {
  const at = new Date(2026, 9, 24, 9, 5, 7);
  assert.equal(pastedName({ name: "image.png", type: "image/png" }, at), "pasted-2026-10-24-090507.png");
  assert.equal(pastedName({ name: "", type: "image/jpeg" }, at), "pasted-2026-10-24-090507.jpg");
  assert.equal(pastedName({ name: "plan.pdf", type: "application/pdf" }, at), "plan.pdf");
});

test("a file over the limit, or empty, is refused before it goes up", () => {
  assert.equal(tooBig({ size: MAX_FILE_BYTES }), null);
  assert.match(tooBig({ size: MAX_FILE_BYTES + 1 }), /at most 10 MB/);
  assert.match(tooBig({ size: 0 }), /empty/);
});
