// The front page's moving picture: a made-up team app with arugula.stream
// in it. Chat with live faces beside each message, and a room of people:
// some as characters (scripted rigs through the real stage), some as line
// drawings of a face (a little generator, through the real outline
// renderer). A cursor waves at someone, they wave back, and the tile turns
// into a call. Nothing here touches a camera.

import { useEffect, useRef, useState } from "preact/hooks";
import { characterOf } from "@arugula/presence-core/avatar/characters/index";
import { animate } from "@arugula/presence-core/avatar/stage";
import { neutral, waving, type Rig } from "@arugula/presence-core/avatar/rig";
import { draw } from "@arugula/presence-core/outline/render";
import { palette } from "@arugula/presence-core/outline/classes";
import type { Frame } from "@arugula/presence-core/outline/frame";
import type { Polyline } from "@arugula/presence-core/outline/encode";

/** A burst of speech: the jaw opens and closes in syllables, with pauses. */
function talk(t: number, seed: number): number {
  const phrase = Math.sin(t * 0.9 + seed) > -0.2 ? 1 : 0;
  return phrase * Math.max(0, Math.sin(t * 11 + seed * 3)) * (0.45 + 0.35 * Math.sin(t * 3.1 + seed));
}

const blink = (t: number, seed: number) => {
  const p = ((t + seed * 1.7) % 3.7) / 3.7;
  return p > 0.95 ? Math.sin(((p - 0.95) / 0.05) * Math.PI) : 0;
};

type Script = (t: number) => Rig;

const SCRIPTS: Record<string, Script> = {
  /** Talking, nodding along to herself. */
  talker: (t) =>
    Object.assign(neutral(), {
      face: true, jawOpen: talk(t, 0), smile: 0.35, blinkL: blink(t, 0), blinkR: blink(t, 0),
      pitch: Math.sin(t * 1.3) * 0.08, yaw: Math.sin(t * 0.5) * 0.2, browUp: Math.max(0, Math.sin(t * 0.9)) * 0.4,
    }),
  /** Laughing at something, then back to grinning. */
  laugher: (t) => {
    const lol = Math.max(0, Math.sin(t * 0.7 + 1)) ** 2;
    return Object.assign(neutral(), {
      face: true, smile: 0.5 + lol * 0.5, jawOpen: lol * (0.5 + 0.3 * Math.abs(Math.sin(t * 9))),
      blinkL: Math.max(blink(t, 2), lol * 0.6), blinkR: Math.max(blink(t, 2), lol * 0.6), roll: lol * 0.12, pitch: lol * 0.1,
      browUp: lol * 0.5,
    });
  },
  /** Waving hello. */
  waver: (t) => {
    const on = Math.sin(t * 0.45) > 0.1;
    return Object.assign(neutral(), {
      face: true, smile: 0.75, blinkL: blink(t, 4), blinkR: blink(t, 4), yaw: -0.1,
      wristL: on ? [0.95 + Math.sin(t * 7) * 0.08, -0.5] : null,
      handL: on ? { angle: -Math.PI / 2 + Math.sin(t * 7) * 0.35 } : null,
    });
  },
  /** Heads down, typing. */
  typer: (t) =>
    Object.assign(neutral(), {
      face: true, pitch: -0.18, lookY: -0.7, lookX: Math.sin(t * 0.8) * 0.3, blinkL: blink(t, 6), blinkR: blink(t, 6),
      wristL: [0.3, 0.72 + Math.abs(Math.sin(t * 13)) * 0.03], handL: { angle: -0.5 },
      wristR: [-0.3, 0.72 + Math.abs(Math.sin(t * 11 + 1)) * 0.03], handR: { angle: Math.PI + 0.5 },
    }),
};

function Character({ character, colour, script, size, away = false, wave = false }: { character: number; colour: number; script?: string; size: "bust" | "head"; away?: boolean; wave?: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const waveRef = useRef(wave);
  waveRef.current = wave;
  useEffect(
    () =>
      animate(ref.current!, {
        size,
        character: characterOf,
        source: () => {
          const t = performance.now() / 1000;
          const rig = script && !away ? SCRIPTS[script](t) : null;
          return { rig: waveRef.current ? waving(rig, t) : rig, look: { character, colour }, away };
        },
      }),
    [character, colour, script, size, away],
  );
  return <canvas ref={ref} />;
}

/** A line drawing of a face, as the outline camera would trace one: a few
 * dozen strokes with a hand-traced wobble, blinking and talking. */
function face(t: number, seed: number, o: { glasses?: boolean; hair: "short" | "long" | "curly" }): Frame {
  const W = 192, H = 144;
  const lines: Polyline[] = [];
  const classes: number[] = [];
  // A traced line wobbles a little from frame to frame.
  const step = Math.floor(t * 6);
  let n = seed * 997 + step * 31;
  const jit = () => ((n = (n * 16807) % 2147483647) / 2147483647 - 0.5) * 1.4;
  const yaw = Math.sin(t * 0.4 + seed) * 7;
  const cx = 96 + yaw * 0.4, cy = 60;
  const add = (pts: [number, number][], cls: number) => {
    lines.push(pts.flatMap(([x, y]) => [Math.round(x + jit()), Math.round(y + jit())]));
    classes.push(cls);
  };
  const arc = (x: number, y: number, rx: number, ry: number, a0: number, a1: number, k = 18): [number, number][] =>
    Array.from({ length: k + 1 }, (_, i) => {
      const a = a0 + ((a1 - a0) * i) / k;
      return [x + Math.cos(a) * rx, y + Math.sin(a) * ry];
    });
  // The head, ears, neck and shoulders: the person's.
  add(arc(cx, cy, 26, 33, Math.PI * 0.08, Math.PI * 0.92, 20), 15);
  add(arc(cx - 26, cy + 2, 4, 7, Math.PI * 0.5, Math.PI * 1.5, 6), 15);
  add(arc(cx + 26, cy + 2, 4, 7, -Math.PI * 0.5, Math.PI * 0.5, 6), 15);
  add([[cx - 11, cy + 31], [cx - 12, cy + 44]], 15);
  add([[cx + 11, cy + 31], [cx + 12, cy + 44]], 15);
  add(arc(96, 150, 70, 26, Math.PI * 1.08, Math.PI * 1.92, 20), 15);
  // Hair.
  if (o.hair === "short") add(arc(cx, cy - 4, 27, 30, Math.PI * 1.02, Math.PI * 1.98, 18), 15);
  if (o.hair === "long") {
    add(arc(cx, cy - 2, 29, 32, Math.PI * 0.95, Math.PI * 2.05, 20), 15);
    add([[cx - 28, cy + 2], [cx - 30, cy + 30], [cx - 26, cy + 44]], 15);
    add([[cx + 28, cy + 2], [cx + 30, cy + 30], [cx + 26, cy + 44]], 15);
  }
  if (o.hair === "curly")
    for (let i = 0; i < 7; i++) add(arc(cx - 24 + i * 8, cy - 26 + Math.abs(i - 3) * 2.5, 6, 5, Math.PI, Math.PI * 2, 6), 15);
  // The face: eyes that blink, brows, a nose, a mouth that talks.
  const b = blink(t, seed);
  const fx = yaw * 0.6;
  for (const s of [-1, 1]) {
    const ex = cx + fx + s * 10, ey = cy - 2;
    add(arc(ex, ey, 5, Math.max(0.6, 3.2 * (1 - b)), 0, Math.PI * 2, 10), 32);
    add([[ex - 6, ey - 8 - Math.max(0, Math.sin(t * 0.9 + seed)) * 2], [ex + 5, ey - 9]], 32);
    if (o.glasses) add(arc(ex, ey, 8, 6.5, 0, Math.PI * 2, 14), 32);
  }
  if (o.glasses) add([[cx + fx - 2, cy - 2], [cx + fx + 2, cy - 2]], 32);
  add([[cx + fx, cy + 2], [cx + fx - 3, cy + 12], [cx + fx + 2, cy + 13]], 32);
  const open = talk(t, seed) * 6;
  add(arc(cx + fx, cy + 19, 9, 2 + open, 0, Math.PI, 12), 32);
  if (open > 1) add(arc(cx + fx, cy + 19, 9, 1.2, Math.PI, Math.PI * 2, 10), 32);
  return { w: W, h: H, lines, classes };
}

function LineFace({ seed, look }: { seed: number; look: { glasses?: boolean; hair: "short" | "long" | "curly" } }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current!;
    const tick = () => draw(c, face(performance.now() / 1000, seed, look), { color: palette(c, true) });
    tick();
    // An outline camera sends a handful of frames a second.
    const id = setInterval(tick, 160);
    return () => clearInterval(id);
  }, [seed]);
  return <canvas ref={ref} />;
}

interface Person {
  name: string;
  kind: "avatar" | "lines";
  character?: number;
  colour?: number;
  script?: string;
  away?: boolean;
  seed?: number;
  look?: { glasses?: boolean; hair: "short" | "long" | "curly" };
}

const PEOPLE: Person[] = [
  { name: "Maya", kind: "avatar", character: 2, colour: 0, script: "talker" },
  { name: "Theo", kind: "lines", seed: 3, look: { hair: "short", glasses: true } },
  { name: "Ines", kind: "avatar", character: 3, colour: 2, script: "laugher" },
  { name: "Sam", kind: "lines", seed: 7, look: { hair: "curly" } },
  { name: "Ravi", kind: "avatar", character: 1, colour: 4, script: "typer" },
  { name: "June", kind: "avatar", character: 2, colour: 3, away: true },
];

/** Who the cursor waves at (Ravi, a character, who waves back). */
const WAVED = 4;

function Face({ p, size, wave = false }: { p: Person; size: "bust" | "head"; wave?: boolean }) {
  return p.kind === "lines" ? (
    <LineFace seed={p.seed!} look={p.look!} />
  ) : (
    <Character character={p.character!} colour={p.colour!} script={p.script} size={size} away={p.away} wave={wave} />
  );
}

const CHAT: { who: number; text: string }[] = [
  { who: 0, text: "pushed the new onboarding copy, can someone look?" },
  { who: 2, text: "on it 👀" },
  { who: 4, text: "the checkout test is green again" },
];

/** The story, in seconds of a 12 s loop. */
function phase(t: number): "idle" | "aim" | "wave" | "ask" | "yes" | "call" {
  const s = t % 12;
  if (s < 2) return "idle";
  if (s < 3.4) return "aim";
  if (s < 4.2) return "wave";
  if (s < 5.4) return "ask";
  if (s < 7.6) return "yes";
  return "call";
}

export function FakeRoom() {
  const [t, setT] = useState(0);
  useEffect(() => {
    const t0 = performance.now();
    const id = setInterval(() => setT((performance.now() - t0) / 1000), 100);
    return () => clearInterval(id);
  }, []);
  const ph = phase(t);
  const callSecs = Math.max(0, Math.floor((t % 12) - 7.6));
  return (
    <div class="fake" aria-label="An example: a team app with live presence in it" role="img">
      <div class="fake-chrome">
        <i />
        <i />
        <i />
        <span>acme · #design</span>
      </div>
      <div class="fake-body">
        <div class="fake-chat">
          {CHAT.map((m) => (
            <div class="fake-msg">
              <span class="fake-av">
                <Face p={PEOPLE[m.who]} size="head" />
              </span>
              <span>
                <b>{PEOPLE[m.who].name}</b>
                {m.text}
              </span>
            </div>
          ))}
          <div class="fake-input">Message #design</div>
        </div>
        <div class="fake-people">
          {PEOPLE.map((p, i) => (
            <div class={`fake-tile${i === WAVED && ph === "call" ? " calling" : ""}${p.away ? " away" : ""}`}>
              <Face p={p} size="bust" wave={i === WAVED && ph === "yes"} />
              <span class="fake-name">
                {p.name}
                {i === WAVED && ph === "call" ? <em>talking · 0:0{callSecs}</em> : p.away ? <em class="dim">away</em> : null}
              </span>
              {i === WAVED && (ph === "aim" || ph === "wave" || ph === "ask") && <span class={ph === "wave" ? "fake-wave pressed" : "fake-wave"}>Wave</span>}
            </div>
          ))}
          <span class={`fake-cursor ${ph}`} aria-hidden="true" />
        </div>
      </div>
      {(ph === "ask" || ph === "yes") && (
        <div class="fake-toast">
          {ph === "ask" ? (
            <span>
              Waving at <b>Ravi</b>…
            </span>
          ) : (
            <span>
              <b>Ravi</b> waved back. Joining the call…
            </span>
          )}
        </div>
      )}
    </div>
  );
}
