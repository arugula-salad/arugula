// Rigged avatars (S44): a rig comes back from its bytes, the solver reads
// head turns, blendshapes and arms the way bust space says, and smoothing
// and idling behave. Run with `node --experimental-strip-types`.
import { test } from "node:test";
import assert from "node:assert/strict";
import { approach, elbowFor, idle, neutral, packRig, unpackRig, withElbows } from "../src/shared/avatar/rig.ts";
import { OneEuro, handOf, solve } from "../src/shared/avatar/solve.ts";

const near = (a, b, eps = 0.03) => assert.ok(Math.abs(a - b) <= eps, `${a} ≉ ${b}`);

test("a rig comes back from its bytes, close enough to draw the same", () => {
  const r = Object.assign(neutral(), {
    face: true, yaw: 0.5, pitch: -0.2, roll: 0.1, x: 0.12, y: -0.08,
    blinkL: 1, lookX: -0.6, lookY: 0.4, smile: 0.7, jawOpen: 0.3, cheekPuff: 0.2,
    elbowL: [0.9, 0.3], wristL: [1.1, -0.5], elbowR: null, wristR: null,
  });
  const b = packRig(r, { character: 3, colour: 2 });
  assert.ok(b.length <= 30, `${b.length} bytes`);
  const { rig, look } = unpackRig(b);
  assert.deepEqual(look, { character: 3, colour: 2 });
  assert.equal(rig.face, true);
  for (const k of ["yaw", "pitch", "roll", "x", "y", "blinkL", "lookX", "lookY", "smile", "jawOpen", "cheekPuff"]) near(rig[k], r[k]);
  near(rig.elbowL[0], 0.9, 0.05);
  near(rig.wristL[1], -0.5, 0.05);
  assert.equal(rig.elbowR, null);
  assert.equal(packRig(neutral(), { character: 1, colour: 0 }).length, 21, "no arms, no arm bytes");
  assert.throws(() => unpackRig(Uint8Array.from([1, 2, 3])), /not one/);
});

test("the head turns the way the face transform says", () => {
  // Turned toward the image's right (the person's left) by 0.4 rad: the
  // forward axis (third column) leans to +x.
  const y = 0.4;
  const m = [Math.cos(y), 0, -Math.sin(y), 0, 0, 1, 0, 0, Math.sin(y), 0, Math.cos(y), 0, 0, 0, 0, 1];
  const r = solve({ matrix: m, shapes: null, face: null, pose: null });
  assert.equal(r.face, true);
  near(r.yaw, 0.4);
  near(r.pitch, 0);
  near(r.roll, 0);
});

test("blendshapes: blinks stretch, the person's left eye looking out is a look to the viewer's right", () => {
  const r = solve({
    matrix: null,
    face: null,
    pose: null,
    shapes: { eyeBlinkLeft: 0.6, eyeBlinkRight: 0.1, eyeLookOutLeft: 0.7, eyeLookInRight: 0.7, mouthSmileLeft: 0.5, mouthSmileRight: 0.5, jawOpen: 0.5 },
  });
  near(r.blinkL, 1);
  near(r.blinkR, 0);
  assert.ok(r.lookX > 0.9, `lookX ${r.lookX}`);
  assert.ok(r.smile > 0.6 && r.jawOpen > 0.6);
});

test("arms in bust space: shoulders 1 apart, the person's left on +x, unseen joints null", () => {
  const pose = [];
  pose[11] = { x: 0.6, y: 0.7, visibility: 0.99 }; // left shoulder, image right
  pose[12] = { x: 0.4, y: 0.7, visibility: 0.99 };
  pose[13] = { x: 0.7, y: 0.5, visibility: 0.9 }; // left elbow, raised
  pose[14] = { x: 0.3, y: 0.9, visibility: 0.2 }; // right elbow, not seen
  pose[15] = { x: 0.72, y: 0.3, visibility: 0.9 };
  pose[16] = { x: 0.3, y: 1.0, visibility: 0.9 };
  const face = [];
  face[1] = { x: 0.5, y: 0.52 };
  const r = solve({ matrix: null, shapes: null, face, pose, aspect: 1 });
  near(r.elbowL[0], 1);
  near(r.elbowL[1], -1);
  near(r.wristL[1], -2);
  assert.equal(r.elbowR, null);
  assert.ok(r.wristR, "a wrist counts without its elbow");
  near(r.x, 0);
});

test("One-Euro smooths jitter when still and keeps up when moving", () => {
  const f = new OneEuro();
  let out = 0;
  for (let i = 0; i < 60; i++) out = f.filter(i % 2 ? 0.05 : -0.05, i / 30);
  assert.ok(Math.abs(out) < 0.04, `jitter left: ${out}`);
  const g = new OneEuro();
  for (let i = 0; i < 30; i++) out = g.filter(i / 30, i / 30);
  assert.ok(out > 0.8, `lag: ${out}`);
});

test("idle blinks now and then and dozes away; approach eases toward a rig", () => {
  let blinked = false;
  for (let t = 0; t < 10; t += 0.02) if (idle(t, false).rig.blinkL > 0.5) blinked = true;
  assert.ok(blinked, "a blink in 10 s");
  assert.equal(idle(3, true).rig.blinkL, 1);
  const want = Object.assign(neutral(), { yaw: 1, elbowL: [1, 0] });
  const half = approach(neutral(), want, 1 / 20);
  assert.ok(half.yaw > 0.3 && half.yaw < 0.8, `${half.yaw}`);
  assert.deepEqual(half.elbowL, [1, 0], "an arm coming into view snaps");
});

/** A hand from `w`, its middle knuckle `dx, dy` away. */
function handMarks(w, dx = 0, dy = -0.08) {
  const m = [];
  for (let i = 0; i < 21; i++) m[i] = { x: w.x, y: w.y };
  m[9] = { x: w.x + dx, y: w.y + dy };
  return m;
}

test("a hand points wrist to knuckles", () => {
  near(handOf(handMarks({ x: 0.5, y: 0.8 }), 1).angle, -Math.PI / 2);
  near(handOf(handMarks({ x: 0.5, y: 0.8 }, 0.08, 0), 1).angle, 0);
});

test("a hand goes to the nearer pose wrist's side, and stands in for a lost wrist", () => {
  const pose = [];
  pose[11] = { x: 0.6, y: 0.7, visibility: 1 };
  pose[12] = { x: 0.4, y: 0.7, visibility: 1 };
  const r = solve({ matrix: null, shapes: null, face: null, pose, hands: [handMarks({ x: 0.75, y: 0.5 })], aspect: 1 });
  assert.ok(r.handL && !r.handR, "on the person's left (the image's right)");
  assert.ok(r.wristL && !r.elbowL, "the hand's wrist; the elbow is the drawing's");
  // 0.25 right of the shoulders' middle, which are 0.2 apart.
  near(r.wristL[0], 1.25, 0.05);
});

test("hands survive the wire", () => {
  const r = Object.assign(neutral(), { handL: { angle: -1.2 }, handR: null });
  const back = unpackRig(packRig(r, { character: 2, colour: 1 })).rig;
  near(back.handL.angle, -1.2);
  assert.equal(back.handR, null);
});

test("shoulders: a tilt and a turn come with where they are, a shrug lifts them, and they survive the wire", () => {
  const body = (shY) => {
    const pose = [];
    pose[7] = { x: 0.56, y: 0.42, visibility: 1 }; // left ear
    pose[8] = { x: 0.44, y: 0.42, visibility: 1 };
    pose[11] = { x: 0.6, y: shY, visibility: 1 };
    pose[12] = { x: 0.4, y: shY, visibility: 1 };
    return pose;
  };
  const calib = {};
  const rest = solve({ matrix: null, shapes: null, face: null, pose: body(0.7), aspect: 1 }, calib);
  near(rest.shoulderL[0], 0.5);
  near(rest.shoulderL[1], 0);
  const shrug = solve({ matrix: null, shapes: null, face: null, pose: body(0.62), aspect: 1 }, calib);
  assert.ok(shrug.shoulderL[1] < -0.1 && shrug.shoulderR[1] < -0.1, `shrug ${shrug.shoulderL}`);
  // Turned: the world landmarks put the left shoulder further back.
  const world = [];
  world[11] = { x: 0.15, y: 0, z: 0.1 };
  world[12] = { x: -0.15, y: 0, z: -0.1 };
  const turned = solve({ matrix: null, shapes: null, face: null, pose: body(0.7), poseWorld: world, aspect: 1 }, {});
  assert.ok(turned.shoulderL[0] < 0.45, `turned span ${turned.shoulderL}`);
  const back = unpackRig(packRig(shrug, { character: 1, colour: 0 })).rig;
  near(back.shoulderL[1], shrug.shoulderL[1], 0.05);
  near(back.shoulderR[0], shrug.shoulderR[0], 0.05);
});

test("an elbow the camera missed goes where the arm bends: outward and down, bones their length", () => {
  const s = [0.5, 0];
  for (const w of [[0.9, -0.5], [0.4, 0.6], [1.2, 0.2], [0.3, -0.7]]) {
    const e = elbowFor(s, w, 1);
    near(Math.hypot(e[0] - s[0], e[1] - s[1]), 0.55, 0.01);
    const reach = Math.hypot(w[0] - s[0], w[1] - s[1]);
    if (reach < 1.04) near(Math.hypot(w[0] - e[0], w[1] - e[1]), 0.5, 0.01);
    assert.ok(e[0] - s[0] + 0.6 * (e[1] - s[1]) > -0.2, `bends outward/down: ${e}`);
  }
  // The right arm bends the other way.
  assert.ok(elbowFor([-0.5, 0], [-0.4, 0.6], -1)[0] < -0.5);
  const r = withElbows(Object.assign(neutral(), { wristL: [0.9, -0.5] }));
  assert.ok(r.elbowL && !r.elbowR);
});
