// Your camera as outline frames (S43): video only, scaled to a small
// grid on a canvas nobody sees, traced (./encode) and packed (./frame) at
// a steady cadence. The pixels stay in this page; what leaves is a frame.
//
// With labelling on (./labels, loaded only then), each line also says what
// it traces: you, your chair, your eyes. The models look at the frame at
// twice the grid's size, which a face needs. With depth on (./depth, also
// loaded only then), far lines draw fainter, depth jumps count as edges,
// and a label well behind you is dropped (encode's byDepth). Depth runs
// beside the frames, a couple of times a second.
//
// A frame that barely changed isn't sent (a still room costs nothing),
// but one goes at least every KEEP so the people looking know you're
// still there. A hidden page sends nothing: the timer waits for it.

import { byDepth, trace } from "./encode";
import type { Depth } from "./depth";
import type { Labeller, Labels } from "./labels";
import { FACE } from "./classes";
import { encodeFrame, pack, type Frame } from "./frame";

/** The grid: 4:3, small enough that a frame is a few KB. */
export const GRID_W = 192;
export const GRID_H = 144;
/** Below this much of the edge map changing, a frame is the same frame. */
const STILL = 0.02;
/** The longest a still camera goes without a frame. */
const KEEP = 4_000;

export interface FrameStats {
  /** On the wire (deflated when smaller). */
  bytes: number;
  /** Before deflate. */
  raw: number;
  lines: number;
  points: number;
  /** Trace + pack, in ms. */
  ms: number;
  /** Labelling, in ms; null while it's off or loading. */
  labelMs: number | null;
  /** The last depth estimate, in ms (it runs beside the frames); null
   * while it's off or loading. */
  depthMs: number | null;
  /** Whether it was sent, or held back as still. */
  sent: boolean;
}

export interface Camera {
  /** Frames a second. */
  setRate(fps: number): void;
  setDetail(d: number): void;
  setLabel(on: boolean): void;
  setDepth(on: boolean): void;
  stop(): void;
}

export interface CameraOptions {
  fps: number;
  detail: number;
  /** Say what each line traces (./labels). */
  label: boolean;
  /** Labelling is loading, on, or failed (with why). */
  onLabelState(state: "loading" | "on" | "off", error?: string): void;
  /** Estimate depth (./depth). */
  depth: boolean;
  onDepthState(state: "loading" | "on" | "off", error?: string): void;
  /** Every frame traced (sent or not), for your own tile. */
  onFrame(f: Frame, s: FrameStats): void;
  /** A frame worth sending, as wire bytes. */
  send(bytes: Uint8Array): void;
  onError(e: Error): void;
}

export async function startCamera(o: CameraOptions): Promise<Camera> {
  const stream = await navigator.mediaDevices.getUserMedia({
    video: { width: { ideal: 640 }, height: { ideal: 480 }, frameRate: { ideal: 15 } },
    audio: false,
  });
  const video = document.createElement("video");
  video.muted = true;
  video.playsInline = true;
  video.srcObject = stream;
  await video.play();
  const canvas = document.createElement("canvas");
  canvas.width = GRID_W;
  canvas.height = GRID_H;
  const g = canvas.getContext("2d", { willReadFrequently: true })!;
  // What the models see: the same crop, at twice the grid.
  const big = document.createElement("canvas");
  big.width = GRID_W * 2;
  big.height = GRID_H * 2;
  const bg = big.getContext("2d")!;
  let labeller: Labeller | null = null;
  let lastLabels: Labels | undefined;
  let lastLabelMs = 0;
  let labelIn = 0;
  let depther: Depth | null = null;
  let depthing = false;
  let lastDepth: Uint8Array | undefined;
  let depthMs: number | null = null;
  let depthBusy = false;
  let depthAt = 0;
  const setDepth = (on: boolean) => {
    depthing = on;
    if (!on) {
      depther?.close();
      depther = null;
      lastDepth = undefined;
      depthMs = null;
      o.onDepthState("off");
      return;
    }
    if (depther) return;
    o.onDepthState("loading");
    void import("./depth")
      .then((m) => m.startDepth())
      .then((d) => {
        if (!depthing || stopped) return d.close();
        depther = d;
        o.onDepthState("on");
      })
      .catch((e: Error) => {
        depthing = false;
        o.onDepthState("off", `Depth didn't start: ${e.message}`);
      });
  };
  /** A new estimate when the last is done and at least half a second (and
   * three of its own run times) old: the room doesn't move much. */
  const freshenDepth = () => {
    if (!depther || depthBusy || performance.now() - depthAt < Math.max(500, (depthMs ?? 0) * 3)) return;
    depthBusy = true;
    const d0 = performance.now();
    // Its own copy: the frame canvas is drawn over before it finishes.
    const snap = document.createElement("canvas");
    snap.width = big.width;
    snap.height = big.height;
    snap.getContext("2d")!.drawImage(big, 0, 0);
    void depther
      .estimate(snap, GRID_W, GRID_H)
      .then((z) => {
        if (depther) lastDepth = z;
        depthMs = performance.now() - d0;
      })
      .catch((e: Error) => o.onError(e))
      .finally(() => {
        depthBusy = false;
        depthAt = performance.now();
      });
  };
  let labelling = false;
  const setLabel = (on: boolean) => {
    labelling = on;
    if (!on) {
      labeller?.close();
      labeller = null;
      o.onLabelState("off");
      return;
    }
    if (labeller) return;
    o.onLabelState("loading");
    void import("./labels")
      .then((m) => m.startLabeller())
      .then((l) => {
        if (!labelling || stopped) return l.close();
        labeller = l;
        o.onLabelState("on");
      })
      .catch((e: Error) => {
        labelling = false;
        o.onLabelState("off", `Labelling didn't start: ${e.message}`);
      });
  };

  let fps = o.fps;
  let detail = o.detail;
  let prev: Uint8Array | undefined;
  let lastSent = 0;
  let timer: number | undefined;
  let stopped = false;

  const tick = async () => {
    timer = undefined;
    if (stopped) return;
    if (document.hidden || !video.videoWidth) return schedule();
    try {
      // Cover the grid: crop the video to 4:3 around its middle.
      const vw = video.videoWidth, vh = video.videoHeight;
      const want = GRID_W / GRID_H;
      const sw = vw / vh > want ? vh * want : vw;
      const sh = vw / vh > want ? vh : vw / want;
      bg.drawImage(video, (vw - sw) / 2, (vh - sh) / 2, sw, sh, 0, 0, big.width, big.height);
      g.drawImage(big, 0, 0, GRID_W, GRID_H);
      // Labelling runs on this thread: where it's slow (no GPU), it runs
      // every few frames and the frames between keep its last answer, so
      // it never takes more than about a third of the time between frames.
      let labelMs: number | null = null;
      let labels = lastLabels;
      if (labeller && --labelIn <= 0) {
        const l0 = performance.now();
        labels = lastLabels = labeller.label(big, GRID_W, GRID_H);
        labelMs = lastLabelMs = performance.now() - l0;
        labelIn = Math.max(1, Math.ceil(lastLabelMs / ((1000 / fps) * 0.35)));
      } else if (labeller) labelMs = lastLabelMs;
      if (!labeller) lastLabels = undefined;
      freshenDepth();
      const depth = depther ? lastDepth : undefined;
      const mask = labels && depth ? byDepth(labels.mask, depth) : labels?.mask;
      const t0 = performance.now();
      const img = g.getImageData(0, 0, GRID_W, GRID_H);
      const t = trace(img.data, GRID_W, GRID_H, { detail, prev, labels: mask, depth });
      prev = t.edges;
      const face = labels?.face ?? [];
      const frame = {
        w: t.w,
        h: t.h,
        lines: [...t.lines, ...face],
        classes: [...t.classes, ...face.map(() => FACE)],
        depths: [...t.depths, ...face.map(() => 255)],
      };
      const raw = pack(frame).length;
      const now = Date.now();
      const send = t.changed >= STILL || now - lastSent >= KEEP;
      const wire = send ? await encodeFrame(frame) : null;
      const ms = performance.now() - t0;
      const points = frame.lines.reduce((s, l) => s + l.length / 2, 0);
      o.onFrame(frame, { bytes: wire?.length ?? 0, raw, lines: frame.lines.length, points, ms, labelMs, depthMs: depther ? depthMs : null, sent: send });
      if (wire) {
        lastSent = now;
        o.send(wire);
      }
    } catch (e) {
      o.onError(e as Error);
    }
    schedule();
  };
  const schedule = () => {
    if (stopped || timer !== undefined) return;
    timer = window.setTimeout(() => void tick(), document.hidden ? 1000 : 1000 / fps);
  };
  schedule();
  if (o.label) setLabel(true);
  if (o.depth) setDepth(true);

  return {
    setRate(f) {
      fps = f;
    },
    setDetail(d) {
      detail = d;
    },
    setLabel,
    setDepth,
    stop() {
      stopped = true;
      if (timer !== undefined) clearTimeout(timer);
      labeller?.close();
      labeller = null;
      depther?.close();
      depther = null;
      for (const t of stream.getTracks()) t.stop();
      video.srcObject = null;
    },
  };
}
