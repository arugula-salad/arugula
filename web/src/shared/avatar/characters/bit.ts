import { HEAD_R, HEAD_Y, type Character, type Stage } from "../character";
import { drawHand } from "../hand";
import { SHOULDER_L, SHOULDER_R, type Hand, type Point } from "../rig";

const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));

// A continuous casing around the two links, with a soft elbow bend.
function arm(g: CanvasRenderingContext2D, side: 1 | -1, shoulder: NonNullable<Point>, lift: number, elbow: Point, wrist: Point, hand: Hand | null, s: Stage) {
  // An arm the camera doesn't see isn't drawn: a bust, until a hand shows.
  if (!wrist) return;
  const sx = shoulder[0], sy = shoulder[1] - lift;
  const dx = sx - side * 0.5;
  const ex = elbow ? clamp(elbow[0], -0.82, 0.82) : side * 0.62 + dx;
  const ey = elbow ? clamp(elbow[1], -0.65, 0.55) : 0.27 + sy;
  // Retarget the reach to Bit's compact proportions, reserving room for
  // extended fingers even when the tracked wrist is outside the tile.
  const reach = 0.85 - (hand ? Math.abs(Math.cos(hand.angle)) * 0.15 : 0);
  const wx = wrist ? clamp(wrist[0], -reach, reach) : side * 0.64 + dx;
  const wy = wrist ? clamp(wrist[1], -0.92, 0.5) : 0.49 + sy;
  const upper = Math.atan2(ey - sy, ex - sx);
  const lower = Math.atan2(wy - ey, wx - ex);
  const ux = -Math.sin(upper) * 0.055, uy = Math.cos(upper) * 0.055;
  const vx = -Math.sin(lower) * 0.045, vy = Math.cos(lower) * 0.045;
  g.beginPath();
  g.moveTo(sx + ux, sy + uy);
  g.lineTo(ex * 0.8 + sx * 0.2 + ux, ey * 0.8 + sy * 0.2 + uy);
  g.quadraticCurveTo(ex + ux, ey + uy, ex + vx, ey + vy);
  g.lineTo(wx + vx, wy + vy);
  g.quadraticCurveTo(wx + Math.cos(lower) * 0.045, wy + Math.sin(lower) * 0.045, wx - vx, wy - vy);
  g.lineTo(ex - vx, ey - vy);
  g.quadraticCurveTo(ex - ux, ey - uy, ex * 0.8 + sx * 0.2 - ux, ey * 0.8 + sy * 0.2 - uy);
  g.lineTo(sx - ux, sy - uy);
  g.closePath();
  g.fill();
  g.stroke();
  g.lineWidth = s.stroke * 0.6;
  g.beginPath();
  g.arc(ex, ey, 0.028, 0, Math.PI * 2);
  g.stroke();
  g.lineWidth = s.stroke;
  drawHand(g, [wx, wy], hand, side, { size: 0.15, fill: s.fill, stroke: s.colour });
}

export const bit: Character = {
  id: 1,
  name: "Bit",
  draw(g, rig, s) {
    const stroke = g.lineWidth;
    const lift = s.breathe * 0.018;
    const shoulderL = rig.shoulderL ?? SHOULDER_L;
    const shoulderR = rig.shoulderR ?? SHOULDER_R;
    const r = HEAD_R;
    const turn = Math.sin(rig.yaw);
    const pitch = Math.sin(rig.pitch);
    const width = r * (1 - 0.16 * Math.abs(turn));
    const height = r * (0.88 - Math.abs(pitch) * 0.08);
    g.save();
    g.lineJoin = g.lineCap = "round";
    g.strokeStyle = s.colour;
    g.fillStyle = s.fill;

    if (s.size === "bust") {
      const cx = (shoulderL[0] + shoulderR[0]) / 2;
      const cy = (shoulderL[1] + shoulderR[1]) / 2;
      const span = shoulderL[0] - shoulderR[0];
      const slope = shoulderL[1] - shoulderR[1];
      // The flexible neck meets the rotated head's underside, not its
      // unrotated centre, so a leaning head stays seated on the shoulders.
      const nx = rig.x + Math.sin(rig.roll) * height;
      const ny = HEAD_Y + rig.y + Math.cos(rig.roll) * height;
      g.beginPath();
      g.moveTo(cx - 0.105 * span, cy - 0.105 * slope + 0.02 - lift);
      g.quadraticCurveTo(nx - 0.11, cy - 0.14, nx - 0.10, ny - 0.035);
      g.lineTo(nx + 0.10, ny - 0.035);
      g.quadraticCurveTo(nx + 0.11, cy - 0.14, cx + 0.105 * span, cy + 0.105 * slope + 0.02 - lift);
      g.closePath();
      g.fill();
      g.stroke();

      // Rounded shoulders and a lightly tapered chassis, with breathing
      // room underneath the bust and its relaxed hands.
      // Map the original shoulder line onto the rig's; at rest this is
      // the identity, preserving the finished casing and its breathing.
      g.save();
      g.transform(span, slope, 0, 1, cx, cy);
      g.beginPath();
      g.moveTo(-0.29, -0.12 - lift);
      g.quadraticCurveTo(-0.50, -0.12 - lift, -0.51, 0.09 - lift);
      g.lineTo(-0.44, 0.61 - lift);
      g.quadraticCurveTo(-0.43, 0.76 - lift, -0.28, 0.76 - lift);
      g.lineTo(0.28, 0.76 - lift);
      g.quadraticCurveTo(0.43, 0.76 - lift, 0.44, 0.61 - lift);
      g.lineTo(0.51, 0.09 - lift);
      g.quadraticCurveTo(0.50, -0.12 - lift, 0.29, -0.12 - lift);
      g.closePath();
      g.fill();
      g.stroke();
      // One small terminal slot is the chassis's only ornament.
      // A narrower, asymmetric shoulder span turns the chest details
      // toward the nearer side, in addition to narrowing the casing.
      g.translate(clamp(cx * 2, -0.2, 0.2) * clamp(1 - span, 0, 1), 0);
      g.lineWidth = stroke * 0.6;
      g.beginPath();
      g.roundRect(-0.21, 0.19 - lift, 0.42, 0.16, 0.045);
      g.moveTo(-0.12, 0.27 - lift);
      g.lineTo(0.025, 0.27 - lift);
      g.stroke();
      g.restore();
    }

    g.save();
    g.translate(rig.x, HEAD_Y + rig.y);
    g.rotate(-rig.roll);
    // Just the antenna sways: a quiet, slow signal of life.
    const antennaX = Math.sin(s.t * 1.6) * 0.014 - turn * 0.07;
    g.beginPath();
    g.moveTo(-turn * 0.07, -height + 0.025);
    g.quadraticCurveTo(antennaX, -height - 0.045, antennaX, -height - 0.09);
    g.stroke();
    g.beginPath();
    g.arc(antennaX, -height - 0.105, 0.032, 0, Math.PI * 2);
    g.fill();
    g.stroke();

    g.beginPath();
    g.roundRect(-width, -height, width * 2, height * 2, 0.145);
    g.fill();
    g.stroke();
    // The receding side of the housing becomes visible in a turn.
    if (Math.abs(turn) > 0.08) {
      const back = turn > 0 ? -1 : 1;
      const seam = back * (width - 0.02 - Math.abs(turn) * 0.075);
      g.lineWidth = stroke * 0.6;
      g.strokeStyle = s.dim;
      g.beginPath();
      g.moveTo(seam - back * 0.025, -height + 0.06);
      g.quadraticCurveTo(seam, -height + 0.09, seam, -height + 0.16);
      g.lineTo(seam, height - 0.16);
      g.quadraticCurveTo(seam, height - 0.09, seam - back * 0.025, height - 0.06);
      g.stroke();
    }

    // Compress the far half and slide the whole face across the shell.
    // Conservative travel keeps brows and mouth inside even at full yaw.
    g.save();
    g.translate(turn * 0.115, -pitch * 0.105);
    for (let side = -1; side <= 1; side += 2) {
      const far = 1 - Math.max(0, -side * turn) * 0.22;
      const blink = clamp(side === 1 ? rig.blinkL : rig.blinkR, 0, 1);
      const eyeX = side * 0.165 * (1 - Math.abs(turn) * 0.13);
      const eyeY = -0.055;
      const joy = rig.smile * (1 - rig.browDown);
      const eyeW = 0.071 * far;
      const open = (1 - blink) * (1 - joy * 0.28);
      g.strokeStyle = s.ink;
      g.lineWidth = stroke * 1.2;
      g.beginPath();
      if (open > 0.45) {
        const px = eyeX + rig.lookX * 0.035;
        const py = eyeY - rig.lookY * 0.028;
        g.moveTo(px, py - 0.064 * open * far);
        g.lineTo(px, py + 0.064 * open * far);
      } else {
        // Happy lower lids arch upward; a blink/doze closes softly down.
        const bend = joy > 0.3 ? -0.065 : 0.035;
        g.moveTo(eyeX - eyeW, eyeY + 0.01);
        g.quadraticCurveTo(eyeX, eyeY + bend, eyeX + eyeW, eyeY + 0.01);
      }
      g.stroke();
      if (joy > 0.3 && open > 0.45) {
        g.lineWidth = stroke * 0.6;
        g.beginPath();
        g.moveTo(eyeX - eyeW, eyeY + 0.10);
        g.quadraticCurveTo(eyeX, eyeY + 0.10 - joy * 0.045, eyeX + eyeW, eyeY + 0.10);
        g.stroke();
      }
      g.strokeStyle = s.colour;
      g.lineWidth = stroke * 0.6;
      const browY = -0.205 - rig.browUp * 0.055 + rig.browDown * 0.025;
      g.beginPath();
      g.moveTo(eyeX + side * 0.082 * far, browY - rig.browDown * 0.035);
      g.quadraticCurveTo(eyeX, browY - 0.018,
        eyeX - side * 0.072 * far, browY - rig.browInnerUp * 0.075 + rig.browDown * 0.08);
      g.stroke();
      if (rig.cheekPuff > 0.1) {
        g.beginPath();
        g.moveTo(side * 0.27, 0.065);
        g.quadraticCurveTo(side * (0.27 + rig.cheekPuff * 0.04), 0.115, side * 0.27, 0.16);
        g.stroke();
      }
    }

    const oo = Math.max(rig.funnel, rig.pucker);
    const mouthY = 0.155;
    g.lineWidth = stroke;
    g.strokeStyle = s.ink;
    g.beginPath();
    if (oo > 0.3) {
      g.ellipse(0, mouthY, 0.066 - rig.pucker * 0.018, 0.055 + rig.funnel * 0.037 + rig.jawOpen * 0.025, 0, 0, Math.PI * 2);
    } else if (rig.jawOpen > 0.12) {
      const w = 0.105 + rig.smile * 0.07;
      const h = 0.045 + rig.jawOpen * 0.11;
      g.moveTo(-w, mouthY - 0.035);
      g.quadraticCurveTo(0, mouthY - 0.01 + rig.smile * 0.025, w, mouthY - 0.035);
      g.bezierCurveTo(w, mouthY + h, -w, mouthY + h, -w, mouthY - 0.035);
      g.closePath();
    } else {
      const w = 0.10 + rig.smile * 0.065;
      const corner = mouthY - rig.smile * 0.028 + rig.browDown * 0.04;
      g.moveTo(-w, corner);
      g.quadraticCurveTo(0, mouthY + 0.015 + rig.smile * 0.12 - rig.browDown * 0.065, w, corner);
    }
    g.stroke();
    g.restore();

    if (s.away) {
      const phase = (s.t * 0.23) % 1;
      const zx = width - 0.045 + phase * 0.075;
      const zy = -height + 0.03 - phase * 0.12;
      g.strokeStyle = s.dim;
      g.lineWidth = stroke * 0.6;
      g.globalAlpha *= Math.sin(phase * Math.PI);
      g.beginPath();
      g.moveTo(zx, zy);
      g.lineTo(zx + 0.075, zy);
      g.lineTo(zx, zy + 0.085);
      g.lineTo(zx + 0.075, zy + 0.085);
      g.stroke();
    }
    g.restore();

    // Forearms and hands sit in front, including the thoughtful chin pose.
    if (s.size === "bust") {
      g.lineWidth = stroke;
      g.strokeStyle = s.colour;
      arm(g, -1, shoulderR, lift, rig.elbowR, rig.wristR, rig.handR, s);
      arm(g, 1, shoulderL, lift, rig.elbowL, rig.wristL, rig.handL, s);
    }
    g.restore();
  },
};
