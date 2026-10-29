// @arugula/presence-server: sign an arugula.stream room token on your
// server (PROTOCOL.md, "Room token"). An HS256 JWT with the claims in a
// fixed order and compact JSON, so every arugula SDK makes the same bytes
// for the same inputs (../vectors.json). node:crypto only.

import { createHmac } from "node:crypto";

const ROOM = /^[A-Za-z0-9_.:-]{1,128}$/;
const SUB = /^[A-Za-z0-9_.:@-]{1,128}$/;
// Control characters (and the two line separators some JSON encoders
// escape) aren't names: refusing them keeps every SDK's JSON identical.
const CONTROL = /[\u0000-\u001f\u007f\u2028\u2029]/;
const MAX_TTL = 86400;
const HEADER = '{"alg":"HS256","typ":"JWT"}';

const b64 = (s) => Buffer.from(s).toString("base64url");

/** A room token for `user` in `room`, signed with your app's secret. */
export function createRoomToken({ app, secret, room, user, look, ttl = 3600, now }) {
  if (typeof app !== "string" || !ROOM.test(app)) throw new Error("app: your app's id, 1–128 of A–Z a–z 0–9 _ . : -");
  if (typeof secret !== "string" || !secret) throw new Error("secret: your app's secret");
  if (typeof room !== "string" || !ROOM.test(room)) throw new Error("room: 1–128 of A–Z a–z 0–9 _ . : -");
  const id = user?.id, name = user?.name;
  if (typeof id !== "string" || !SUB.test(id)) throw new Error("user.id: 1–128 of A–Z a–z 0–9 _ . : @ -");
  if (typeof name !== "string" || !name || [...name].length > 80 || CONTROL.test(name)) throw new Error("user.name: 1–80 characters, no control characters");
  if (!Number.isInteger(ttl) || ttl < 1 || ttl > MAX_TTL) throw new Error("ttl: 1–86400 seconds");
  const byte = (v) => Number.isInteger(v) && v >= 0 && v <= 255;
  if (look != null && !(byte(look.character) && byte(look.colour))) throw new Error("look: {character, colour}, each 0–255");
  const at = now ?? Math.floor(Date.now() / 1000);
  let claims = `{"app":${JSON.stringify(app)},"room":${JSON.stringify(room)},"sub":${JSON.stringify(id)},"name":${JSON.stringify(name)},"exp":${at + ttl}`;
  if (look != null) claims += `,"look":{"character":${look.character},"colour":${look.colour}}`;
  claims += "}";
  const body = `${b64(HEADER)}.${b64(claims)}`;
  return `${body}.${createHmac("sha256", secret).update(body).digest("base64url")}`;
}
