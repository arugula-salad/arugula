// Outline frames (S43): a drawn picture traces to lines along its edges,
// and a frame comes back from its bytes as it went in, raw or deflated.
// Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { simplify, trace } from "../src/shared/outline/encode.ts";
import { decodeFrame, encodeFrame, pack, unpack } from "../src/shared/outline/frame.ts";

const W = 160, H = 120;

/** A gray room with a lighter disc (a head) and a dark bar (a desk). */
function picture(dx = 0) {
  const px = new Uint8ClampedArray(W * H * 4);
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      let v = 70;
      if ((x - 80 - dx) ** 2 + (y - 50) ** 2 < 28 ** 2) v = 190;
      if (y > 95 && x > 20 && x < 140) v = 20;
      const i = (y * W + x) * 4;
      px[i] = px[i + 1] = px[i + 2] = v;
      px[i + 3] = 255;
    }
  }
  return px;
}

test("a disc and a bar trace to lines along their edges, and nowhere else", () => {
  const t = trace(picture(), W, H);
  assert.ok(t.lines.length >= 2, `lines: ${t.lines.length}`);
  for (const l of t.lines) {
    for (let i = 0; i < l.length; i += 2) {
      const [x, y] = [l[i], l[i + 1]];
      const r = Math.hypot(x - 80, y - 50);
      const nearDisc = Math.abs(r - 28) < 4;
      const nearBar = Math.abs(y - 95.5) < 4 || (y > 92 && (Math.abs(x - 20.5) < 4 || Math.abs(x - 139.5) < 4));
      assert.ok(nearDisc || nearBar, `stray point ${x},${y}`);
    }
  }
  assert.equal(t.changed, 1);
});

test("a flat image has no lines", () => {
  const px = new Uint8ClampedArray(W * H * 4).fill(128);
  assert.deepEqual(trace(px, W, H).lines, []);
});

test("the same picture twice hasn't changed; a moved one has", () => {
  const a = trace(picture(), W, H);
  assert.ok(trace(picture(), W, H, { prev: a.edges }).changed < 0.01);
  assert.ok(trace(picture(10), W, H, { prev: a.edges }).changed > 0.3);
});

test("Douglas–Peucker keeps the corners of a staircase-straight line", () => {
  assert.deepEqual(simplify([0, 0, 1, 0, 2, 0, 3, 1, 4, 1, 5, 1, 5, 2, 5, 3], 0.8), [0, 0, 5, 1, 5, 3]);
});

test("a frame comes back from its bytes, raw and deflated", async () => {
  const t = trace(picture(), W, H);
  const f = { w: W, h: H, lines: t.lines, classes: t.lines.map((_, i) => i % 3) };
  assert.deepEqual(unpack(pack(f)), f);
  const wire = await encodeFrame(f);
  assert.ok(wire.length <= pack(f).length + 1);
  assert.deepEqual(await decodeFrame(wire), f);
  const tiny = { w: 4, h: 4, lines: [[0, 0, 3, 3]], classes: [32] };
  const w2 = await encodeFrame(tiny);
  assert.equal(w2[0], 0, "a tiny frame stays raw");
  assert.deepEqual(await decodeFrame(w2), tiny);
  assert.throws(() => unpack(Uint8Array.from([9, 1, 1, 0])), /version/);
  assert.throws(() => unpack(Uint8Array.from([1, 4, 4, 1, 3, 0])), /cut short/);
});

test("a big negative delta survives", () => {
  const f = { w: 255, h: 255, lines: [[254, 254, 0, 0, 200, 3]], classes: [0] };
  assert.deepEqual(unpack(pack(f)), f);
});

test("a version 1 frame (no classes) still reads", () => {
  // v1: version, w, h, count 1, points 2, x0 y0, dx dy (zigzag 2 = +1)
  assert.deepEqual(unpack(Uint8Array.from([1, 8, 8, 1, 2, 1, 1, 2, 2])), { w: 8, h: 8, lines: [[1, 1, 2, 2]], classes: [0] });
});

test("labels colour a line by what it runs along, and split it where that changes", () => {
  // The disc is a person (15), the desk a chair (9).
  const labels = new Uint8Array(W * H);
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      if ((x - 80) ** 2 + (y - 50) ** 2 < 29 ** 2) labels[y * W + x] = 15;
      else if (y > 93 && x > 18 && x < 142) labels[y * W + x] = 9;
    }
  }
  const t = trace(picture(), W, H, { labels, detail: 1 });
  assert.equal(t.classes.length, t.lines.length);
  const seen = new Set(t.classes);
  assert.ok(seen.has(15) && seen.has(9), `classes: ${[...seen]}`);
  t.lines.forEach((l, i) => {
    const ys = l.filter((_, k) => k % 2 === 1);
    if (t.classes[i] === 15) assert.ok(Math.max(...ys) < 85, "a person line stays on the disc");
    if (t.classes[i] === 9) assert.ok(Math.min(...ys) > 88, "a chair line stays on the desk");
  });
  assert.ok(trace(picture(), W, H).classes.every((c) => c === 0), "no labels, no classes");
});
