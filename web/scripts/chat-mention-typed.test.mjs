// Which mention lines a message may show (#1167): a machine the text names,
// in any case or as the machine is shown, and what the chip says. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { typesMachine } from "../src/shared/chat/mention-typed.ts";

test("a typed mention is found by handle, by display name and in any case", () => {
  assert.equal(typesMachine("hey @geek/keymaster look", "geek"), true);
  assert.equal(typesMachine("@GEEK look", "geek"), true);
  assert.equal(typesMachine("hey @aarons-mac-mini.local/ui-fixes", "Aarons-Mac-mini.local"), true);
  assert.equal(typesMachine("hey @Aarons-Mac-mini.local, look", "Aarons-Mac-mini.local"), true);
  assert.equal(typesMachine("@My-Mini hi", "My Mini"), true);
  assert.equal(typesMachine("ok @geek.", "geek"), true);
});

test("a message that doesn't name the machine doesn't type it", () => {
  assert.equal(typesMachine("no mention at all", "geek"), false);
  assert.equal(typesMachine("", "geek"), false);
  assert.equal(typesMachine("mail me at bob@geek.com", "geek"), false);
  assert.equal(typesMachine("@geeky thing", "geek"), false);
  assert.equal(typesMachine("@geek-two hi", "geek"), false);
  assert.equal(typesMachine("geek without the at", "geek"), false);
});
