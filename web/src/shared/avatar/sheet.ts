// web/avatar-sheet.html (S44): each character down, each pose across, as
// a bust and as a 36 px chat avatar, through the real stage. For tuning
// characters by eye; not shipped.

import { CHARACTERS, characterOf } from "./characters";
import { neutral, type Rig } from "./rig";
import { animate } from "./stage";

const pose = (name: string, f: (r: Rig, t: number) => void) => ({ name, f });
const POSES = [
  pose("idle", () => {}),
  pose("neutral", (r) => void (r.face = true)),
  pose("blink L", (r) => Object.assign(r, { face: true, blinkL: 1 })),
  pose("look ↗", (r) => Object.assign(r, { face: true, lookX: 0.9, lookY: 0.7 })),
  pose("smile", (r) => Object.assign(r, { face: true, smile: 1, browUp: 0.4 })),
  pose("laugh", (r) => Object.assign(r, { face: true, smile: 0.9, jawOpen: 0.85, blinkL: 0.6, blinkR: 0.6 })),
  pose("oo", (r) => Object.assign(r, { face: true, funnel: 0.9, pucker: 0.6, browInnerUp: 0.7 })),
  pose("cross", (r) => Object.assign(r, { face: true, browDown: 1, cheekPuff: 0.8 })),
  pose("turn →", (r) => Object.assign(r, { face: true, yaw: 0.6, x: 0.12 })),
  pose("tilt + up", (r) => Object.assign(r, { face: true, roll: 0.35, pitch: 0.35 })),
  // Arms by wrist alone, as a desk camera sees them: the elbows are the
  // stage's (rig's withElbows).
  pose("wave", (r, t) =>
    Object.assign(r, { face: true, smile: 0.7, wristL: [0.95 + Math.sin(t * 6) * 0.12, -0.55], handL: { angle: -Math.PI / 2 + Math.sin(t * 6) * 0.3 } }),
  ),
  pose("chin", (r) => Object.assign(r, { face: true, wristR: [-0.08, -0.45], handR: { angle: -Math.PI / 2 }, pitch: -0.1 })),
  pose("hands up", (r) =>
    Object.assign(r, {
      face: true, jawOpen: 0.5, browUp: 0.9,
      wristL: [0.75, -0.8], handL: { angle: -Math.PI / 2 }, wristR: [-0.75, -0.8], handR: { angle: -Math.PI / 2 },
    }),
  ),
  pose("typing", (r) =>
    Object.assign(r, { face: true, pitch: -0.15, lookY: -0.6, wristL: [0.3, 0.75], handL: { angle: -0.4 }, wristR: [-0.3, 0.75], handR: { angle: Math.PI + 0.4 } }),
  ),
  pose("point", (r) => Object.assign(r, { face: true, browUp: 0.5, wristL: [1.05, -0.15], handL: { angle: -0.2 } })),
  pose("shrug", (r) =>
    Object.assign(r, {
      face: true, browUp: 0.8, browInnerUp: 0.6, smile: 0.2, shoulderL: [0.5, -0.2], shoulderR: [-0.5, -0.2],
      wristL: [1.05, -0.15], handL: { angle: -0.6 },
      wristR: [-1.05, -0.15], handR: { angle: -Math.PI + 0.6 },
    }),
  ),
  pose("lean", (r) => Object.assign(r, { face: true, roll: 0.2, x: 0.1, shoulderL: [0.48, -0.12], shoulderR: [-0.48, 0.12] })),
  pose("turn body", (r) => Object.assign(r, { face: true, yaw: 0.5, x: 0.05, shoulderL: [0.3, 0.02], shoulderR: [-0.42, 0] })),
  pose("away", () => {}),
];

const table = document.getElementById("sheet") as HTMLTableElement;
const head = table.insertRow();
head.insertCell().textContent = "";
for (const p of POSES) {
  const th = document.createElement("th");
  th.textContent = p.name;
  head.appendChild(th);
}
CHARACTERS.forEach((c, ci) => {
  const row = table.insertRow();
  const name = document.createElement("th");
  name.textContent = c.name;
  row.appendChild(name);
  for (const p of POSES) {
    const cell = row.insertCell();
    for (const size of ["bust", "head"] as const) {
      const canvas = document.createElement("canvas");
      canvas.className = size;
      cell.appendChild(canvas);
      animate(canvas, {
        size,
        character: characterOf,
        source: () => {
          const t = performance.now() / 1000;
          if (p.name === "idle" || p.name === "away") return { rig: null, look: { character: c.id, colour: ci }, away: p.name === "away" };
          const r = neutral();
          p.f(r, t);
          return { rig: r, look: { character: c.id, colour: ci }, away: false };
        },
      });
    }
  }
});
