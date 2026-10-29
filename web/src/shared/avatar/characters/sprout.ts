import { HEAD_R, HEAD_Y, type Character } from "../character";

export const sprout: Character = {
  id: 2,
  name: "Sprout",
  draw(g, rig, s) {
    const stroke = g.lineWidth;
    const lift = -0.02 * s.breathe;
    const r = HEAD_R;
    const yaw = Math.sin(rig.yaw);
    const faceX = yaw * 0.6 * r;
    const faceY = -Math.sin(rig.pitch) * 0.5 * r;
    const headW = r * (0.88 + 0.12 * Math.cos(rig.yaw));
    const oo = 1 - (1 - rig.funnel) * (1 - rig.pucker);

    g.save();
    g.lineJoin = "round";
    g.lineCap = "round";
    g.strokeStyle = s.colour;
    g.fillStyle = s.fill;

    // A soft stem neck, and a little pear-shaped leafy bust.
    g.beginPath();
    g.moveTo(rig.x - 0.105, HEAD_Y + rig.y + r * 0.8);
    g.quadraticCurveTo(-0.12, -0.25, -0.18, lift);
    g.moveTo(rig.x + 0.105, HEAD_Y + rig.y + r * 0.8);
    g.quadraticCurveTo(0.12, -0.25, 0.18, lift);
    g.moveTo(-0.5, lift);
    g.bezierCurveTo(-0.65, 0.22 + lift, -0.59, 0.63 + lift, -0.48, 0.9 + lift);
    g.quadraticCurveTo(0, 1 + lift, 0.48, 0.9 + lift);
    g.bezierCurveTo(0.59, 0.63 + lift, 0.65, 0.22 + lift, 0.5, lift);
    g.stroke();

    for (let side = -1; side <= 1; side += 2) {
      // L is the person's left: the viewer's right. Missing joints rest
      // independently, so a visible wrist still works without an elbow.
      const elbow = side === 1 ? rig.elbowL : rig.elbowR;
      const wrist = side === 1 ? rig.wristL : rig.wristR;
      const ex = elbow ? elbow[0] : side * 0.66;
      const ey = elbow ? elbow[1] : 0.36 + lift;
      const wx = wrist ? wrist[0] : side * 0.65;
      const wy = wrist ? wrist[1] : 0.72 + lift;
      g.beginPath();
      g.moveTo(side * 0.5, lift);
      g.quadraticCurveTo(side * 0.6, lift + 0.12, ex, ey);
      g.quadraticCurveTo(ex + side * 0.07, (ey + wy) / 2, wx, wy);
      g.stroke();
      g.beginPath();
      g.arc(wx, wy, 0.055, 0, Math.PI * 2);
      g.fill();
      g.stroke();
    }

    // Serrated collar overlaps the shoulders and the tops of the arms.
    g.beginPath();
    g.moveTo(-0.17, -0.08 + lift);
    g.quadraticCurveTo(-0.38, -0.12 + lift, -0.55, 0.03 + lift);
    g.lineTo(-0.4, 0.08 + lift);
    g.lineTo(-0.45, 0.2 + lift);
    g.lineTo(-0.27, 0.14 + lift);
    g.lineTo(-0.24, 0.28 + lift);
    g.lineTo(0, 0.13 + lift);
    g.lineTo(0.24, 0.28 + lift);
    g.lineTo(0.27, 0.14 + lift);
    g.lineTo(0.45, 0.2 + lift);
    g.lineTo(0.4, 0.08 + lift);
    g.lineTo(0.55, 0.03 + lift);
    g.quadraticCurveTo(0.38, -0.12 + lift, 0.17, -0.08 + lift);
    g.quadraticCurveTo(0, 0.03 + lift, -0.17, -0.08 + lift);
    g.closePath();
    g.fill();
    g.stroke();

    g.save();
    g.translate(rig.x, HEAD_Y + rig.y);
    // Canvas +y is down: negative rotation tips the crown to the right.
    g.rotate(-rig.roll);
    for (let side = -1; side <= 1; side += 2) {
      g.save();
      g.translate(side * 0.085, -r * 0.85);
      // Tracking perks up the leaves; idle leaves relax a little.
      g.rotate(side * (rig.face ? 0.36 : 0.52) - rig.roll * 0.18
        + Math.sin(s.t * 2.3 + side) * 0.055);
      g.beginPath();
      g.moveTo(0, 0.025);
      g.lineTo(-0.065, -0.055);
      g.lineTo(-0.025, -0.09);
      g.lineTo(-0.095, -0.15);
      g.lineTo(-0.035, -0.16);
      g.lineTo(-0.075, -0.23);
      g.quadraticCurveTo(-0.045, -0.29, 0, -0.33);
      g.quadraticCurveTo(0.07, -0.27, 0.08, -0.23);
      g.lineTo(0.035, -0.18);
      g.lineTo(0.09, -0.16);
      g.lineTo(0.03, -0.1);
      g.lineTo(0.065, -0.07);
      g.closePath();
      g.fill();
      g.stroke();
      g.lineWidth = stroke * 0.6;
      g.beginPath();
      g.moveTo(0, 0);
      g.quadraticCurveTo(0.012, -0.15, 0, -0.27);
      g.stroke();
      g.restore();
    }

    g.beginPath();
    g.ellipse(0, 0, headW + rig.cheekPuff * 0.045, r, 0, 0, Math.PI * 2);
    g.fill();
    g.stroke();
    g.translate(faceX, faceY);

    for (let side = -1; side <= 1; side += 2) {
      const far = 1 - Math.max(0, -side * yaw) * 0.23;
      const blink = side === 1 ? rig.blinkL : rig.blinkR;
      const eyeX = side * r * 0.37;
      const eyeY = -r * 0.14;
      const ew = r * (s.size === "head" ? 0.235 : 0.22) * far;
      const eh = ew * (1 - blink);
      g.strokeStyle = s.ink;
      g.beginPath();
      if (blink > 0.94) {
        g.moveTo(eyeX - ew, eyeY);
        g.quadraticCurveTo(eyeX, eyeY + 0.035, eyeX + ew, eyeY);
        g.stroke();
      } else {
        g.ellipse(eyeX, eyeY, ew, eh, 0, 0, Math.PI * 2);
        g.stroke();
        // Clip pupils to the independently closing lids.
        g.save();
        g.clip();
        g.beginPath();
        g.ellipse(eyeX + rig.lookX * ew * 0.43,
          eyeY - rig.lookY * eh * 0.4, ew * 0.34, ew * 0.38,
          0, 0, Math.PI * 2);
        g.stroke();
        g.restore();
      }

      g.strokeStyle = s.colour;
      const browY = eyeY - ew - 0.035 - rig.browUp * 0.065 + rig.browDown * 0.035;
      g.beginPath();
      g.moveTo(eyeX + side * ew, browY - rig.browDown * 0.025);
      g.quadraticCurveTo(eyeX, browY - 0.025,
        eyeX - side * ew * 0.8, browY - rig.browInnerUp * 0.075 + rig.browDown * 0.04);
      g.stroke();

      // Two rosy hatch marks, broader and longer as the smile grows.
      g.lineWidth = stroke * 0.6;
      const cheekX = side * (0.24 + rig.cheekPuff * 0.035);
      const cheekY = 0.09;
      for (let mark = 0; mark < 2; mark++) {
        const cx = cheekX + side * mark * (0.022 + rig.smile * 0.012);
        g.beginPath();
        g.moveTo(cx - 0.008, cheekY);
        g.lineTo(cx + 0.008, cheekY + 0.02 + rig.smile * 0.04);
        g.stroke();
      }
      g.lineWidth = stroke;
    }

    const mouthW = (0.105 + rig.smile * 0.065) * (1 - oo) + 0.043 * oo;
    const mouthY = 0.15 + rig.pucker * 0.018;
    const opening = rig.jawOpen * 0.12 * (1 - oo * 0.6) + oo * 0.045;
    const corners = mouthY - rig.smile * 0.045 * (1 - oo);
    g.strokeStyle = s.ink;
    g.beginPath();
    g.moveTo(-mouthW, corners);
    g.bezierCurveTo(-mouthW * 0.5, mouthY - opening,
      mouthW * 0.5, mouthY - opening, mouthW, corners);
    g.bezierCurveTo(mouthW * 0.6, mouthY + 0.02 + opening,
      -mouthW * 0.6, mouthY + 0.02 + opening, -mouthW, corners);
    if (opening < 0.005) {
      // A closed smile is one curve, not a tiny outlined hole.
      g.beginPath();
      g.moveTo(-mouthW, corners);
      g.quadraticCurveTo(0, mouthY + 0.035, mouthW, corners);
    }
    g.stroke();
    g.restore();

    if (s.away) {
      const drift = (s.t * 0.22) % 1;
      const zx = rig.x + r + 0.065 + drift * 0.06;
      const zy = HEAD_Y + rig.y - 0.15 - drift * 0.18;
      g.strokeStyle = s.dim;
      g.lineWidth = stroke * 0.6;
      g.beginPath();
      g.moveTo(zx, zy);
      g.lineTo(zx + 0.085, zy);
      g.lineTo(zx, zy + 0.09);
      g.lineTo(zx + 0.085, zy + 0.09);
      g.stroke();
    }
    g.restore();
  },
};
