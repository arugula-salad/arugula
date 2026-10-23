// A passkey's PRF output as bytes (#764): browsers give ArrayBuffers, some
// password managers' passkeys give views or base64; anything else gives no
// key, so a deploy's approval signs with the browser's own.
// Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { prfBytes } from "../src/shell/e2e/keys.ts";

const bytes = Uint8Array.from({ length: 32 }, (_, i) => i);
const same = (b) => assert.deepEqual(new Uint8Array(b), bytes);

test("an ArrayBuffer, a view and base64 are the same bytes", () => {
  same(prfBytes(bytes.slice().buffer));
  same(prfBytes(bytes.slice()));
  same(prfBytes(new DataView(bytes.slice().buffer)));
  same(prfBytes(Buffer.from(bytes).toString("base64")));
  same(prfBytes(Buffer.from(bytes).toString("base64url")));
});

test("anything else is no output", () => {
  for (const x of [undefined, null, 7, {}, [1, 2], "not base64!", "", new ArrayBuffer(0), { 0: 1, length: 1 }]) {
    assert.equal(prfBytes(x), undefined, String(x));
  }
});
