// The appearance across arugula.io's hosts (shared/appearance-cookie.ts):
// a cookie on arugula.io only, read quietly when it's missing or garbled,
// and the cookie's copy first, then localStorage's. Run with
// `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { cookieDomain, cookieLine, pickStored, readCookie } from "../src/shared/appearance-cookie.ts";

const KEY = "arugula-appearance";
const latte = JSON.stringify({ palette: "latte", finish: "glass", corners: "round" });
const parse = (raw) => {
  try {
    const v = JSON.parse(raw);
    return v && typeof v === "object" ? v : null;
  } catch {
    return null;
  }
};

test("the cookie is arugula.io's and its hosts', nobody else's", () => {
  for (const h of ["control.arugula.io", "app.arugula.io", "arugula.io", "APP.Arugula.IO", "app.arugula.io."]) assert.equal(cookieDomain(h), "arugula.io", h);
  for (const h of ["127.0.0.1", "localhost", "tauri.localhost", "geek.tail1234.ts.net", "evilarugula.io", "arugula.io.example.com", "arugula.run", ""]) {
    assert.equal(cookieDomain(h), null, h);
  }
});

test("its line is on the parent domain, secure, for a year", () => {
  const line = cookieLine(KEY, latte, "arugula.io");
  assert.match(line, /^arugula-appearance=[^;\s",]+; /);
  for (const part of ["Domain=arugula.io", "Path=/", "Max-Age=31536000", "Secure", "SameSite=Lax"]) assert.ok(line.split("; ").includes(part), part);
});

test("what it writes reads back, among other cookies", () => {
  const value = cookieLine(KEY, latte, "arugula.io").split(";")[0];
  assert.equal(readCookie(value, KEY), latte);
  assert.equal(readCookie(`ilg_session=abc; ${value}; other=1`, KEY), latte);
  assert.equal(readCookie(`x${KEY}=nope; ${value}`, KEY), latte);
});

test("a missing or garbled cookie is nothing", () => {
  assert.equal(readCookie("", KEY), null);
  assert.equal(readCookie("other=1", KEY), null);
  assert.equal(readCookie(`${KEY}=%E0%A4%A`, KEY), null);
  // Decodes, but isn't JSON: parse turns it down, and localStorage's copy stands.
  assert.deepEqual(pickStored(readCookie(`${KEY}={oops`, KEY), latte, parse), JSON.parse(latte));
});

test("the cookie's copy comes first, then localStorage's", () => {
  const slate = JSON.stringify({ palette: "slate", finish: "flat", corners: "soft" });
  assert.deepEqual(pickStored(latte, slate, parse), JSON.parse(latte));
  assert.deepEqual(pickStored(null, slate, parse), JSON.parse(slate));
  assert.deepEqual(pickStored("null", slate, parse), JSON.parse(slate));
  assert.equal(pickStored(null, null, parse), null);
  assert.equal(pickStored("garbled", "{also", parse), null);
});
