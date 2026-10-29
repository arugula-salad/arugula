// Every vector in ../../vectors.json, byte for byte, and every invalid
// input refused.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRoomToken } from "../index.js";

const vectors = JSON.parse(readFileSync(new URL("../../vectors.json", import.meta.url), "utf8"));
const opts = (i) => ({
  app: i.app,
  secret: i.secret,
  room: i.room,
  user: { id: i.user_id, name: i.user_name },
  look: i.look ?? undefined,
  ttl: i.ttl ?? undefined,
  now: i.now,
});

for (const v of vectors.valid) test(`makes ${v.name}`, () => assert.equal(createRoomToken(opts(v.input)), v.token));
for (const v of vectors.invalid) test(`refuses ${v.name}`, () => assert.throws(() => createRoomToken(opts(v.input))));

test("now defaults to the clock", () => {
  const t = createRoomToken({ app: "acme", secret: "s", room: "r", user: { id: "u", name: "A" } });
  const exp = JSON.parse(Buffer.from(t.split(".")[1], "base64url")).exp;
  assert.ok(Math.abs(exp - (Math.floor(Date.now() / 1000) + 3600)) <= 2);
});
