import { HEAD_R, HEAD_Y, type Character } from "../character";

export const wisp: Character = {
  id: 3,
  name: "Wisp",
  draw(g, rig, s) {
    const stroke = g.lineWidth;
    const r = HEAD_R;
    const hx = rig.x, hy = HEAD_Y + rig.y;
    const turn = Math.sin(rig.yaw);
    const rx = r * (0.86 + 0.14 * Math.cos(rig.yaw));
    const lift = s.breathe * 0.02;
    const c = Math.cos(rig.roll), sn = Math.sin(rig.roll);
    const cheek = rig.cheekPuff * r * 0.14;
    const edge = rx + cheek;
    const lx = hx - edge * c - r * 0.12 * sn;
    const ly = hy - edge * sn + r * 0.12 * c;
    const dx = hx + edge * c - r * 0.12 * sn;
    const dy = hy + edge * sn + r * 0.12 * c;

    g.save();
    g.lineJoin = "round";
    g.lineCap = "round";
    g.strokeStyle = s.dim;
    g.lineWidth = stroke * 0.6;
    // A quiet breathing halo also distinguishes a guessed, untracked head.
    const glow = rig.face ? 1 : 1 + 0.035 * Math.sin(s.t * 1.6);
    g.save();
    g.translate(hx, hy);
    g.rotate(rig.roll);
    g.beginPath();
    g.ellipse(0, -r * 0.08, (edge + r * 0.17) * glow, r * 1.13 * glow, 0, 0, Math.PI * 2);
    g.stroke();
    g.restore();

    // Build the dome in head space, then flow into the bust in stage space.
    g.strokeStyle = s.colour;
    g.lineWidth = stroke;
    g.beginPath();
    g.save();
    g.translate(hx, hy);
    g.rotate(rig.roll);
    g.moveTo(-edge, r * 0.12);
    g.bezierCurveTo(-edge, -r * 0.56, -rx * 0.63, -r, 0, -r);
    g.bezierCurveTo(rx * 0.68, -r, edge, -r * 0.55, edge, r * 0.12);
    g.restore();
    g.bezierCurveTo(dx, dy + 0.26, 0.49, -0.25 - lift, 0.51, -lift);
    g.bezierCurveTo(0.54, 0.28 - lift, 0.57, 0.61 - lift, 0.66, 0.79 - lift);
    for (let i = 0; i < 6; i++) {
      const x = 0.66 - i * 0.22;
      const wave = Math.sin(s.t * 1.8 + i * 1.1) * 0.018;
      g.bezierCurveTo(x - 0.055, 0.89 - lift + wave, x - 0.165, 0.69 - lift - wave, x - 0.22, 0.79 - lift);
    }
    g.bezierCurveTo(-0.57, 0.61 - lift, -0.54, 0.28 - lift, -0.51, -lift);
    g.bezierCurveTo(-0.49, -0.25 - lift, lx, ly + 0.26, lx, ly);
    g.closePath();
    g.fillStyle = s.fill;
    g.fill();
    g.stroke();

    // L is the person's left: positive x on the canvas. Missing joints
    // rest independently, so a visible wrist still works without its elbow.
    for (let side = -1; side <= 1; side += 2) {
      const elbow = side === 1 ? rig.elbowL : rig.elbowR;
      const wrist = side === 1 ? rig.wristL : rig.wristR;
      const ex = elbow ? elbow[0] : side * 0.64;
      const ey = elbow ? elbow[1] : 0.25 - lift;
      const wx = wrist ? wrist[0] : side * 0.66;
      const wy = wrist ? wrist[1] : 0.49 - lift;
      g.beginPath();
      g.moveTo(side * 0.5, -lift);
      g.quadraticCurveTo(side * 0.5 + (ex - side * 0.5) * 0.55, ey * 0.35 - lift * 0.65, ex, ey);
      g.quadraticCurveTo(ex + (wx - ex) * 0.65, ey + (wy - ey) * 0.4, wx, wy);
      g.stroke();
      g.beginPath();
      g.ellipse(wx, wy, 0.075, 0.09, 0, 0, Math.PI * 2);
      g.fill();
      g.stroke();
    }

    g.save();
    g.translate(hx, hy);
    g.rotate(rig.roll);
    g.translate(turn * r * 0.6, -Math.sin(rig.pitch) * r * 0.5);
    const eyeSpacing = r * 0.36;
    for (let side = -1; side <= 1; side += 2) {
      const blink = side === 1 ? rig.blinkL : rig.blinkR;
      const far = 1 - Math.max(0, -side * turn) * 0.18;
      const x = side * eyeSpacing;
      const y = -r * 0.12;
      const ew = r * 0.15 * far;
      const eh = r * 0.235 * far * (1 - blink);
      g.strokeStyle = s.ink;
      g.lineWidth = stroke;
      g.beginPath();
      if (eh < stroke * 0.4) {
        g.moveTo(x - ew, y);
        g.quadraticCurveTo(x, y + r * 0.09, x + ew, y);
        g.stroke();
      } else {
        g.ellipse(x, y, ew, eh, 0, 0, Math.PI * 2);
        g.stroke();
        // Clip the stroke-only pupil inside its independently closing lid.
        g.save();
        g.clip();
        const px = x + rig.lookX * ew * 0.42;
        const py = y - rig.lookY * eh * 0.4;
        g.beginPath();
        g.moveTo(px, py - eh * 0.26);
        g.lineTo(px, py + eh * 0.26);
        g.stroke();
        g.restore();
      }
      g.strokeStyle = s.colour;
      g.lineWidth = stroke * 0.6;
      const by = y - r * 0.34 - rig.browUp * r * 0.18 + rig.browDown * r * 0.11;
      const inner = by - rig.browInnerUp * r * 0.22 + rig.browDown * r * 0.15;
      g.beginPath();
      g.moveTo(x - side * ew, inner);
      g.quadraticCurveTo(x, by - r * 0.055, x + side * ew, by - rig.browDown * r * 0.06);
      g.stroke();
    }

    const oo = Math.max(rig.funnel, rig.pucker);
    const mw = r * (0.23 + rig.smile * 0.16) * (1 - oo * 0.7);
    const my = r * 0.39 + rig.pucker * r * 0.04;
    const opening = r * (rig.jawOpen * 0.33 * (1 - oo * 0.45) + rig.funnel * 0.13 + rig.pucker * 0.065);
    const curve = rig.smile * r * 0.17 * (1 - oo);
    g.strokeStyle = s.ink;
    g.lineWidth = stroke;
    g.beginPath();
    g.moveTo(-mw, my);
    g.bezierCurveTo(-mw * 0.55, my + curve - opening, mw * 0.55, my + curve - opening, mw, my);
    if (opening > 0.002) {
      g.bezierCurveTo(mw * 0.8, my + curve + opening, -mw * 0.8, my + curve + opening, -mw, my);
      g.closePath();
    }
    g.stroke();
    g.restore();

    if (s.away) {
      const drift = Math.sin(s.t * 1.25);
      const zx = hx + r * 0.77 + drift * 0.018;
      const zy = hy - r * 0.74 - (1 + Math.sin(s.t * 0.9)) * 0.025;
      const z = s.size === "head" ? 0.085 : 0.10;
      g.strokeStyle = s.dim;
      g.lineWidth = stroke * 0.6;
      g.beginPath();
      g.moveTo(zx, zy);
      g.lineTo(zx + z, zy);
      g.lineTo(zx, zy + z);
      g.lineTo(zx + z, zy + z);
      g.stroke();
    }
    g.restore();
  },
};
