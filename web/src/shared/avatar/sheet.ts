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
  pose("wave", (r, t) =>
    Object.assign(r, { face: true, smile: 0.7, elbowL: [0.85, 0.05], wristL: [0.95 + Math.sin(t * 6) * 0.15, -0.55], elbowR: null, wristR: null }),
  ),
  pose("chin", (r) => Object.assign(r, { face: true, elbowR: [-0.45, 0.55], wristR: [-0.08, -0.45], pitch: -0.1 })),
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
