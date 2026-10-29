import { HEAD_R, HEAD_Y, type Character } from "../character";
import type { Point } from "../rig";

// A hollow capsule: the background fill hides the joint behind each segment.
function segment(g: CanvasRenderingContext2D, x: number, y: number, tx: number, ty: number) {
  const length = Math.hypot(tx - x, ty - y);
  g.save();
  g.translate(x, y);
  g.rotate(Math.atan2(ty - y, tx - x));
  g.beginPath();
  g.roundRect(0, -0.065, Math.max(length, 0.13), 0.13, 0.06);
  g.fill();
  g.stroke();
  g.restore();
}

function arm(g: CanvasRenderingContext2D, side: number, lift: number, elbow: Point, wrist: Point) {
  const ex = elbow ? elbow[0] : side * 0.61;
  const ey = elbow ? elbow[1] : 0.34 - lift;
  const wx = wrist ? wrist[0] : ex + side * 0.015;
  const wy = wrist ? wrist[1] : ey + 0.31;
  segment(g, side * 0.5, -lift, ex, ey);
  segment(g, ex, ey, wx, wy);
  g.beginPath();
  g.arc(ex, ey, 0.075, 0, Math.PI * 2);
  g.fill();
  g.stroke();
  g.beginPath();
  g.arc(wx, wy, 0.09, 0, Math.PI * 2);
  g.fill();
  g.stroke();
}

export const bit: Character = {
  id: 1,
  name: "Bit",
  draw(g, rig, s) {
    const stroke = g.lineWidth;
    const lift = s.breathe * 0.02;
    const r = HEAD_R;
    const turn = Math.sin(rig.yaw);
    const width = r * (0.88 + 0.12 * Math.cos(rig.yaw));
    g.save();
    g.lineJoin = g.lineCap = "round";
    g.strokeStyle = s.colour;
    g.fillStyle = s.fill;

    arm(g, 1, lift, rig.elbowL, rig.wristL);
    arm(g, -1, lift, rig.elbowR, rig.wristR);
    // The neck follows the head, while shoulders stay in bust space.
    segment(g, 0, 0.04 - lift, rig.x, HEAD_Y + rig.y + r * 0.7);
    g.beginPath();
    g.roundRect(-0.49, -0.065 - lift, 0.98, 0.96, 0.14);
    g.fill();
    g.stroke();
    g.lineWidth = stroke * 0.6;
    g.beginPath();
    g.roundRect(-0.28, 0.22 - lift, 0.56, 0.31, 0.055);
    g.moveTo(-0.16, 0.34 - lift);
    g.lineTo(0.12, 0.34 - lift);
    g.stroke();
    g.lineWidth = stroke;

    g.translate(rig.x, HEAD_Y + rig.y);
    // Canvas +y is down: negative rotation tips the top toward +x.
    g.rotate(-rig.roll);
    const antennaX = Math.sin(rig.roll) * 0.07 + Math.sin(s.t * 2.2) * 0.015;
    const antennaY = -r * 1.2 + Math.sin(s.t * 2.8) * 0.018;
    g.beginPath();
    g.moveTo(0, -r * 0.7);
    g.quadraticCurveTo(antennaX * 0.4, -r, antennaX, antennaY);
    g.stroke();
    g.beginPath();
    g.arc(antennaX, antennaY, 0.045, 0, Math.PI * 2);
    g.fill();
    g.stroke();

    g.beginPath();
    g.roundRect(-width, -r * 0.76, width * 2, r * 1.52, r * 0.23);
    g.fill();
    g.stroke();
    // A tiny screen cursor is solid when tracking, dashed when guessing.
    g.lineWidth = stroke * 0.6;
    g.beginPath();
    g.moveTo(-width + 0.065, r * 0.57);
    g.lineTo(-width + (rig.face ? 0.15 : 0.095), r * 0.57);
    if (!rig.face) {
      g.moveTo(-width + 0.125, r * 0.57);
      g.lineTo(-width + 0.15, r * 0.57);
    }
    g.stroke();

    // Perspective slides the glyphs without scaling the base stroke.
    const fx = turn * 0.6 * r;
    const fy = -Math.sin(rig.pitch) * 0.5 * r;
    // Keep glyphs on the screen even at the most extreme tracked angles.
    g.save();
    g.beginPath();
    g.roundRect(-width + 0.025, -r * 0.76 + 0.025, width * 2 - 0.05, r * 1.52 - 0.05, 0.06);
    g.clip();
    g.translate(fx, fy);
    for (let side = -1; side <= 1; side += 2) {
      const far = 1 - Math.max(0, -side * turn) * 0.18;
      const blink = side === 1 ? rig.blinkL : rig.blinkR;
      const eyeX = side * r * 0.37;
      const eyeY = -r * 0.12;
      const eyeW = r * 0.14 * far;
      const open = 1 - blink;
      g.lineWidth = stroke;
      g.strokeStyle = s.ink;
      g.beginPath();
      if (blink < 0.95) {
        // Terminal bar pupils shorten as their individual lids descend.
        const px = eyeX + rig.lookX * r * 0.065;
        const py = eyeY - rig.lookY * r * 0.055 * open;
        const h = r * (s.size === "head" ? 0.14 : 0.12) * far * open;
        g.moveTo(px, py - h);
        g.lineTo(px, py + h);
      } else {
        g.moveTo(eyeX - eyeW, eyeY);
        g.quadraticCurveTo(eyeX, eyeY + r * 0.09, eyeX + eyeW, eyeY);
      }
      g.stroke();
      g.strokeStyle = s.colour;
      g.lineWidth = stroke * 0.6;
      const browY = eyeY - r * (0.25 + rig.browUp * 0.16 - rig.browDown * 0.09);
      g.beginPath();
      g.moveTo(eyeX + side * eyeW, browY);
      g.quadraticCurveTo(eyeX, browY - r * 0.04,
        eyeX - side * eyeW, browY - rig.browInnerUp * r * 0.2 + rig.browDown * r * 0.16);
      g.stroke();
      if (rig.cheekPuff > 0) {
        const cx = side * r * (0.57 + rig.cheekPuff * 0.13);
        g.beginPath();
        g.moveTo(cx, r * 0.12);
        g.quadraticCurveTo(cx + side * r * 0.13 * rig.cheekPuff, r * 0.26, cx, r * 0.4);
        g.stroke();
      }
    }

    const oo = Math.max(rig.funnel, rig.pucker);
    const mouthW = r * (0.26 + rig.smile * 0.16) * (1 - oo * 0.75);
    const mouthY = r * (0.31 - rig.pucker * 0.04);
    const gap = r * (rig.jawOpen * 0.28 * (1 - oo * 0.5) + rig.funnel * 0.12 + rig.pucker * 0.075);
    const curve = r * rig.smile * 0.16 * (1 - oo);
    g.lineWidth = stroke;
    g.strokeStyle = s.ink;
    g.beginPath();
    g.moveTo(-mouthW, mouthY);
    g.bezierCurveTo(-mouthW, mouthY + curve - gap, mouthW, mouthY + curve - gap, mouthW, mouthY);
    if (gap > 0.002) {
      g.bezierCurveTo(mouthW, mouthY + curve + gap, -mouthW, mouthY + curve + gap, -mouthW, mouthY);
      g.closePath();
    }
    g.stroke();
    g.restore();

    if (s.away) {
      const drift = Math.sin(s.t * 1.2);
      const zx = r * 0.83 + drift * 0.025;
      const zy = -r * 0.65 - Math.sin(s.t * 1.2 + 0.7) * 0.035;
      g.strokeStyle = s.dim;
      g.lineWidth = stroke * 0.6;
      g.beginPath();
      g.moveTo(zx, zy);
      g.lineTo(zx + 0.1, zy);
      g.lineTo(zx, zy + 0.11);
      g.lineTo(zx + 0.1, zy + 0.11);
      g.stroke();
    }
    g.restore();
  },
};
