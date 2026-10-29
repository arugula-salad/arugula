import { HEAD_R, HEAD_Y, type Character } from "../character";
import { drawHand } from "../hand";
import { SHOULDER_L, SHOULDER_R } from "../rig";

export const sprout: Character = {
  id: 2,
  name: "Sprout",
  draw(g, rig, s) {
    const stroke = g.lineWidth;
    const lift = -0.018 * s.breathe;
    const shoulderL = rig.shoulderL ?? SHOULDER_L;
    const shoulderR = rig.shoulderR ?? SHOULDER_R;
    const shoulderX = (shoulderL[0] + shoulderR[0]) / 2;
    const shoulderY = (shoulderL[1] + shoulderR[1]) / 2;
    const span = shoulderL[0] - shoulderR[0];
    const tilt = shoulderL[1] - shoulderR[1];
    // Follow the shoulder line at the top, easing its lift out at the hem.
    const bust = (x: number, y: number): [number, number] => [
      shoulderX + x * span,
      y + (shoulderY + x * tilt) * Math.max(0, Math.min(1, 1 - y / 0.83)) + lift,
    ];
    const turn = Math.sign(shoulderX) * Math.max(0, 1 - span) * 0.12;
    // Slide the collar's centre on a body turn, keeping its tips attached.
    const collar = (x: number, y: number): [number, number] =>
      bust(x + turn * Math.max(0, 1 - Math.abs(x) / 0.49), y);
    const r = HEAD_R;
    const yaw = Math.sin(rig.yaw);
    const pitch = Math.sin(rig.pitch);
    const headW = r * (1.02 - 0.15 * Math.abs(yaw));
    const faceX = yaw * 0.19;
    const faceY = -pitch * 0.16;
    const oo = 1 - (1 - rig.funnel) * (1 - rig.pucker);
    const smile = rig.smile * (1 - oo);
    // Follow the rotated underside of the head, even when leaning.
    const neckX = rig.x + Math.sin(rig.roll) * r * 0.78;
    const neckY = HEAD_Y + rig.y + Math.cos(rig.roll) * r * 0.78;

    g.save();
    g.lineJoin = "round";
    g.lineCap = "round";
    g.strokeStyle = s.colour;
    g.fillStyle = s.fill;

    // A short, flexible stem inside a compact seed-shaped bust.
    g.beginPath();
    g.moveTo(neckX - 0.105, neckY);
    g.quadraticCurveTo(neckX - 0.12, -0.16 + shoulderY, ...bust(-0.16, 0));
    g.lineTo(...bust(0.16, 0));
    g.quadraticCurveTo(neckX + 0.12, -0.16 + shoulderY, neckX + 0.105, neckY);
    g.closePath();
    g.fill();
    g.stroke();
    g.beginPath();
    g.moveTo(...bust(-0.16, -0.13));
    g.bezierCurveTo(...bust(-0.39, -0.14), ...bust(-0.53, -0.015), ...bust(-0.56, 0.19));
    g.bezierCurveTo(...bust(-0.6, 0.42), ...bust(-0.57, 0.66), ...bust(-0.43, 0.76));
    g.quadraticCurveTo(...bust(0, 0.83), ...bust(0.43, 0.76));
    g.bezierCurveTo(...bust(0.57, 0.66), ...bust(0.6, 0.42), ...bust(0.56, 0.19));
    g.bezierCurveTo(...bust(0.53, -0.015), ...bust(0.39, -0.14), ...bust(0.16, -0.13));
    g.closePath();
    g.fill();
    g.stroke();

    // Two broad collar leaves: a quiet echo of the crown, no serrations.
    g.beginPath();
    g.moveTo(...collar(-0.49, 0.015));
    g.quadraticCurveTo(...collar(-0.24, -0.205), ...collar(0, -0.105));
    g.quadraticCurveTo(...collar(-0.12, 0.14), ...collar(-0.35, 0.18));
    g.quadraticCurveTo(...collar(-0.34, 0.11), ...collar(-0.49, 0.015));
    g.moveTo(...collar(0.49, 0.015));
    g.quadraticCurveTo(...collar(0.24, -0.205), ...collar(0, -0.105));
    g.quadraticCurveTo(...collar(0.12, 0.14), ...collar(0.35, 0.18));
    g.quadraticCurveTo(...collar(0.34, 0.11), ...collar(0.49, 0.015));
    g.fill();
    g.stroke();

    g.save();
    g.translate(rig.x, HEAD_Y + rig.y);
    // Canvas +y is down: negative rotation tips the crown to the right.
    g.rotate(-rig.roll);

    // Low, broad cotyledons remain recognisable inside the chat crop.
    // Only the leaves sway; their bases stay attached to the crown.
    const sway = Math.sin(s.t * 1.7) * 0.018;
    g.beginPath();
    g.moveTo(0, -0.355);
    g.bezierCurveTo(-0.22, -0.33, -0.29, -0.43 + sway, -0.29, -0.525 + sway);
    g.bezierCurveTo(-0.12, -0.545 + sway, -0.025, -0.47, 0, -0.355);
    g.moveTo(0, -0.355);
    g.bezierCurveTo(0.025, -0.48, 0.14, -0.55 - sway, 0.3, -0.52 - sway);
    g.bezierCurveTo(0.29, -0.4 - sway, 0.17, -0.335, 0, -0.355);
    g.fill();
    g.stroke();
    g.lineWidth = stroke * 0.6;
    g.beginPath();
    g.moveTo(-0.2, -0.47 + sway);
    g.quadraticCurveTo(-0.09, -0.43, 0, -0.355);
    g.quadraticCurveTo(0.1, -0.44, 0.21, -0.47 - sway);
    g.stroke();
    g.lineWidth = stroke;

    // A rounded seed, slightly fuller at the cheeks than the crown.
    const cheek = rig.cheekPuff * 0.035;
    g.beginPath();
    g.moveTo(0, -r * 0.87);
    g.bezierCurveTo(headW * 0.65, -r * 0.92, headW + cheek, -0.22, headW + cheek, 0.025);
    g.bezierCurveTo(headW, 0.26, headW * 0.57, r * 0.94, yaw * 0.025, r * 0.94);
    g.bezierCurveTo(-headW * 0.57, r * 0.94, -headW, 0.26, -headW - cheek, 0.025);
    g.bezierCurveTo(-headW - cheek, -0.22, -headW * 0.65, -r * 0.92, 0, -r * 0.87);
    g.closePath();
    g.fill();
    g.stroke();

    // The far temple gives the sliding face a little depth on a turn.
    if (Math.abs(yaw) > 0.12) {
      const farSide = yaw > 0 ? -1 : 1;
      g.lineWidth = stroke * 0.6;
      g.beginPath();
      g.moveTo(farSide * (headW - 0.065), -0.15);
      g.quadraticCurveTo(farSide * (headW - 0.025), -0.015, farSide * (headW - 0.065), 0.12);
      g.stroke();
      g.lineWidth = stroke;
    }
    g.translate(faceX, faceY);

    for (let side = -1; side <= 1; side += 2) {
      const far = 1 - Math.max(0, -side * yaw) * 0.28;
      const blink = side === 1 ? rig.blinkL : rig.blinkR;
      const eyeX = side * 0.17 * (1 - Math.abs(yaw) * 0.12);
      const eyeY = -0.055;
      const ew = 0.092 * far;
      const eh = (0.105 + oo * 0.012) * (1 - blink) * (1 - smile * 0.25);
      const happy = smile * 0.045;
      g.strokeStyle = s.ink;
      g.beginPath();
      if (blink > 0.9 || (smile > 0.6 && blink > 0.45)) {
        g.moveTo(eyeX - ew, eyeY);
        g.quadraticCurveTo(eyeX, eyeY + (s.away ? 0.035 : 0.025) - happy * 1.8, eyeX + ew, eyeY);
        g.stroke();
      } else {
        // A raised lower lid lets the smile reach the eyes.
        g.moveTo(eyeX - ew, eyeY);
        g.bezierCurveTo(eyeX - ew, eyeY - eh * 1.35, eyeX + ew, eyeY - eh * 1.35, eyeX + ew, eyeY);
        g.bezierCurveTo(eyeX + ew, eyeY + eh - happy, eyeX - ew, eyeY + eh - happy, eyeX - ew, eyeY);
        g.closePath();
        g.stroke();
        g.save();
        g.clip();
        g.fillStyle = s.ink;
        g.beginPath();
        g.ellipse(eyeX + rig.lookX * ew * 0.42, eyeY - rig.lookY * eh * 0.4,
          ew * 0.36, 0.047, 0, 0, Math.PI * 2);
        g.fill();
        g.restore();
      }

      // The inner ends drop into a firm frown or rise in surprise.
      const browY = eyeY - 0.155 - rig.browUp * 0.045;
      g.strokeStyle = s.colour;
      g.beginPath();
      g.moveTo(eyeX + side * ew, browY - rig.browDown * 0.015);
      g.quadraticCurveTo(eyeX, browY - 0.025,
        eyeX - side * ew, browY + rig.browDown * 0.095 - rig.browInnerUp * 0.065);
      g.stroke();
    }

    const mouthW = (0.11 + smile * 0.07) * (1 - oo) + 0.058 * oo;
    const mouthY = 0.17 + rig.pucker * 0.012;
    const opening = rig.jawOpen * 0.15;
    g.strokeStyle = s.ink;
    g.beginPath();
    if (oo > 0.45) {
      g.ellipse(0, mouthY, mouthW, 0.065 + opening * 0.4, 0, 0, Math.PI * 2);
    } else if (opening > 0.018) {
      const corners = mouthY - smile * 0.055;
      g.moveTo(-mouthW, corners);
      g.quadraticCurveTo(0, corners - opening * 0.32, mouthW, corners);
      g.bezierCurveTo(mouthW * 0.9, mouthY + opening, -mouthW * 0.9, mouthY + opening, -mouthW, corners);
      g.closePath();
    } else {
      const corners = mouthY - smile * 0.065 + rig.browDown * 0.035;
      g.moveTo(-mouthW, corners);
      g.quadraticCurveTo(0, mouthY + 0.03 + smile * 0.07 - rig.browDown * 0.065, mouthW, corners);
    }
    g.stroke();
    g.restore();

    // Draw arms last so a hand on the chin and raised fists sit in front.
    for (let i = 0; s.size === "bust" && i < 2; i++) {
      const side = i === 0 ? -1 : 1;
      const shoulder = side === 1 ? shoulderL : shoulderR;
      const sx = shoulder[0];
      const sy = shoulder[1] + lift;
      const elbow = side === 1 ? rig.elbowL : rig.elbowR;
      const wrist = side === 1 ? rig.wristL : rig.wristR;
      // An arm the camera doesn't see isn't drawn: a bust, until a hand shows.
      if (!wrist) continue;
      const hand = side === 1 ? rig.handL : rig.handR;
      const angle = hand ? hand.angle : Math.PI / 2;
      // Reserve room for the fingers even when tracking reaches off tile.
      const margin = 0.3 * Math.abs(Math.cos(angle)) + 0.13 * Math.abs(Math.sin(angle));
      const wx = wrist ? Math.max(-0.98 + margin, Math.min(0.98 - margin, wrist[0])) : sx + side * 0.16;
      const wy = wrist ? Math.max(-1.02, Math.min(0.54, wrist[1])) : 0.53 + sy;
      const ex = elbow ? Math.max(-0.86, Math.min(0.86, elbow[0])) : sx + side * 0.15;
      const ey = elbow ? elbow[1] : 0.29 + sy;
      g.strokeStyle = s.colour;
      g.fillStyle = s.fill;
      g.beginPath();
      g.moveTo(sx, sy);
      // Round the elbow, retaining the two limb directions.
      g.lineTo(ex * 0.8 + sx * 0.2, ey * 0.8 + sy * 0.2);
      g.quadraticCurveTo(ex, ey, ex * 0.8 + wx * 0.2, ey * 0.8 + wy * 0.2);
      g.lineTo(wx, wy);
      g.stroke();
      drawHand(g, [wx, wy], hand, side, { size: 0.16, fill: s.fill, stroke: s.colour });
    }

    if (s.away) {
      const drift = (s.t * 0.22) % 1;
      const zx = rig.x + r + 0.035 + drift * 0.045;
      const zy = HEAD_Y + rig.y - 0.16 - drift * 0.18;
      g.strokeStyle = s.dim;
      g.lineWidth = stroke * 0.6;
      g.globalAlpha *= 1 - drift * 0.65;
      g.beginPath();
      g.moveTo(zx, zy);
      g.lineTo(zx + 0.075, zy);
      g.lineTo(zx, zy + 0.08);
      g.lineTo(zx + 0.075, zy + 0.08);
      g.stroke();
    }
    g.restore();
  },
};
