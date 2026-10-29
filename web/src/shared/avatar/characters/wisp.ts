import { HEAD_R, HEAD_Y, type Character } from "../character";
import { drawHand } from "../hand";

export const wisp: Character = {
  id: 3,
  name: "Wisp",
  draw(g, rig, s) {
    const stroke = g.lineWidth;
    const r = HEAD_R;
    const lift = s.breathe * 0.018;
    const hx = rig.x, hy = HEAD_Y + rig.y - lift;
    const turn = Math.sin(rig.yaw), pitch = Math.sin(rig.pitch);
    const rx = r * (1 - 0.16 * Math.abs(turn));
    const ry = r * (1 - 0.07 * Math.abs(pitch));
    const edge = rx + rig.cheekPuff * 0.035;
    const crown = turn * 0.035;
    // Positive rig roll tips the crown toward the viewer's right.
    const roll = -rig.roll;
    const c = Math.cos(roll), sn = Math.sin(roll);
    const joinY = r * 0.18;
    const lx = hx - edge * c - joinY * sn;
    const ly = hy - edge * sn + joinY * c;
    const dx = hx + edge * c - joinY * sn;
    const dy = hy + edge * sn + joinY * c;
    const flutter = Math.sin(s.t * 1.5) * 0.012;

    g.save();
    g.lineJoin = "round";
    g.lineCap = "round";
    g.strokeStyle = s.colour;
    g.fillStyle = s.fill;
    g.lineWidth = stroke;

    // One continuous hood: the tilted dome flows back to fixed shoulders,
    // so leaning never opens a seam between the head and the little cloak.
    g.beginPath();
    g.save();
    g.translate(hx, hy);
    g.rotate(roll);
    g.moveTo(-edge, joinY);
    g.bezierCurveTo(-edge - 0.015, -ry * 0.63, crown - rx * 0.62, -ry, crown, -ry);
    g.bezierCurveTo(crown + rx * 0.64, -ry, edge + 0.015, -ry * 0.63, edge, joinY);
    g.restore();
    g.bezierCurveTo(dx - sn * 0.2, dy + c * 0.2, 0.46, -0.23 - lift, 0.5, -lift);
    g.bezierCurveTo(0.53, 0.26 - lift, 0.59, 0.51 - lift, 0.64, 0.69 - lift);
    // Three soft lobes, with just the centre tip moving in the idle breeze.
    g.bezierCurveTo(0.51, 0.8 - lift, 0.41, 0.8 - lift, 0.3, 0.69 - lift);
    g.bezierCurveTo(0.16, 0.85 - lift + flutter, -0.16, 0.85 - lift + flutter, -0.3, 0.69 - lift);
    g.bezierCurveTo(-0.41, 0.8 - lift, -0.51, 0.8 - lift, -0.64, 0.69 - lift);
    g.bezierCurveTo(-0.59, 0.51 - lift, -0.53, 0.26 - lift, -0.5, -lift);
    g.bezierCurveTo(-0.46, -0.23 - lift, lx - sn * 0.2, ly + c * 0.2, lx, ly);
    g.closePath();
    g.fill();
    g.stroke();

    g.save();
    g.translate(hx, hy);
    g.rotate(roll);
    // A single quiet rim gives the hood depth. It shortens continuously
    // as this side turns toward us, without flipping during an idle sway.
    const rimEnd = -ry * (0.1 + Math.max(0, -turn) * 0.35);
    g.strokeStyle = s.dim;
    g.lineWidth = stroke * 0.6;
    g.beginPath();
    g.moveTo(crown - rx * 0.13, -ry * 0.85);
    g.bezierCurveTo(-rx * 0.68, -ry * 0.85, -rx * 0.88, -ry * 0.43, -(edge - 0.06), rimEnd);
    g.stroke();

    g.translate(turn * r * 0.43, -pitch * r * 0.37);
    const oo = Math.max(rig.funnel, rig.pucker);
    const joy = rig.smile * (1 - rig.browDown);
    for (let side = -1; side <= 1; side += 2) {
      const blink = side === 1 ? rig.blinkL : rig.blinkR;
      const far = 1 - Math.max(0, -side * turn) * 0.23;
      const x = side * r * 0.39 * (1 - Math.abs(turn) * 0.12);
      const y = -r * 0.09 + side * turn * 0.009;
      const ew = r * 0.19 * far;
      const eh = r * (0.255 + oo * 0.025) * far * (1 - blink) * (1 - joy * 0.18);
      g.strokeStyle = s.ink;
      g.lineWidth = stroke;
      g.beginPath();
      if (blink > 0.86 || (joy > 0.6 && blink > 0.45)) {
        g.moveTo(x - ew, y);
        g.quadraticCurveTo(x, y + (s.away ? 0.035 : -0.045 * joy + 0.016 * (1 - joy)), x + ew, y);
        g.stroke();
      } else {
        // The lower lid rises with a smile; the pupil stays inside the eye
        // even while looking up through a partially closed lid.
        const bottom = eh * (1 - joy * 0.35);
        g.moveTo(x - ew, y);
        g.bezierCurveTo(x - ew, y - eh * 1.33, x + ew, y - eh * 1.33, x + ew, y);
        g.bezierCurveTo(x + ew, y + bottom * 1.33, x - ew, y + bottom * 1.33, x - ew, y);
        g.closePath();
        g.stroke();
        g.save();
        g.clip();
        g.fillStyle = s.ink;
        g.beginPath();
        g.ellipse(x + rig.lookX * ew * 0.4, y - rig.lookY * eh * 0.38, ew * 0.3, Math.max(0.012, eh * 0.43), 0, 0, Math.PI * 2);
        g.fill();
        g.restore();
      }

      // Low inner brows and a lowered mouth make a frown read at chat size.
      const by = y - 0.165 - rig.browUp * 0.045 + rig.browDown * 0.045;
      const inner = by - rig.browInnerUp * 0.065 + rig.browDown * 0.052;
      g.strokeStyle = s.colour;
      g.lineWidth = stroke * (rig.browDown > 0.5 ? 1 : 0.6);
      g.beginPath();
      g.moveTo(x - side * ew, inner);
      g.quadraticCurveTo(x, by - 0.022, x + side * ew, by - rig.browDown * 0.025);
      g.stroke();
    }

    const my = r * 0.41;
    const mw = r * (0.24 + joy * 0.18);
    g.strokeStyle = s.ink;
    g.lineWidth = stroke;
    g.beginPath();
    if (oo > 0.25) {
      // A round, upright "oo", distinct from the broad open laugh.
      g.ellipse(0, my + 0.008, r * (0.12 - rig.pucker * 0.025), r * (0.13 + rig.funnel * 0.055 + rig.jawOpen * 0.07), 0, 0, Math.PI * 2);
    } else if (rig.jawOpen > 0.12) {
      const open = r * (0.065 + rig.jawOpen * 0.27);
      g.moveTo(-mw, my - joy * 0.025);
      g.quadraticCurveTo(0, my - open * 0.4 + joy * 0.035, mw, my - joy * 0.025);
      g.bezierCurveTo(mw * 0.85, my + open * 1.25, -mw * 0.85, my + open * 1.25, -mw, my - joy * 0.025);
      g.closePath();
    } else {
      const curve = 0.018 + joy * 0.115 - rig.browDown * 0.075;
      g.moveTo(-mw, my - joy * 0.025);
      g.quadraticCurveTo(0, my + curve, mw, my - joy * 0.025);
    }
    g.stroke();
    g.restore();

    // Hands sit in front of the cloak (and face for the thinking pose).
    // Fit the whole hand inside the tile, including an extended index.
    for (let side = -1; side <= 1; side += 2) {
      const elbow = side === 1 ? rig.elbowL : rig.elbowR;
      const wrist = side === 1 ? rig.wristL : rig.wristR;
      const hand = side === 1 ? rig.handL : rig.handR;
      const angle = hand ? hand.angle : Math.PI / 2;
      const reach = 0.16 * (1.9 * Math.abs(Math.cos(angle)) + 0.75 * Math.abs(Math.sin(angle)));
      const limit = 0.99 - reach;
      const rawX = wrist ? wrist[0] : side * 0.67;
      const wx = Math.max(-limit, Math.min(limit, rawX));
      const wy = wrist ? Math.max(-0.85, Math.min(0.51, wrist[1])) : 0.42 - lift;
      let ex = elbow ? elbow[0] : side * 0.63;
      const ey = elbow ? elbow[1] : 0.22 - lift;
      // Keep the elbow's relation to its shoulder as the reach is shortened.
      if (wx !== rawX && Math.abs(rawX - side * 0.5) > 0.01) {
        ex = side * 0.5 + (ex - side * 0.5) * (wx - side * 0.5) / (rawX - side * 0.5);
      }
      ex = Math.max(-0.92, Math.min(0.92, ex));
      g.strokeStyle = s.colour;
      g.lineWidth = stroke;
      g.beginPath();
      g.moveTo(side * 0.5, -lift);
      g.quadraticCurveTo(side * 0.5 + (ex - side * 0.5) * 0.65, -lift, (side * 0.5 + ex) / 2, (ey - lift) / 2);
      g.quadraticCurveTo(ex, ey, (ex + wx) / 2, (ey + wy) / 2);
      g.quadraticCurveTo(wx - Math.cos(angle) * 0.08, wy - Math.sin(angle) * 0.08, wx, wy);
      g.stroke();
      drawHand(g, [wx, wy], hand, side as 1 | -1, { size: 0.16, kind: "fingers", fill: s.fill, stroke: s.colour });
    }

    if (s.away) {
      const zx = hx + 0.31 + Math.sin(s.t * 0.9) * 0.014;
      const zy = hy - 0.32 - (1 + Math.sin(s.t * 0.9)) * 0.025;
      const z = s.size === "head" ? 0.075 : 0.09;
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
