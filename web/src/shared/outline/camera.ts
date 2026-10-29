// Your camera as outline frames (S43): video only, scaled to a small
// grid on a canvas nobody sees, traced (./encode) and packed (./frame) at
// a steady cadence. The pixels stay in this page; what leaves is a frame.
//
// A frame that barely changed isn't sent (a still room costs nothing),
// but one goes at least every KEEP so the people looking know you're
// still there. A hidden page sends nothing: the timer waits for it.

import { trace } from "./encode";
import { encodeFrame, pack, type Frame } from "./frame";

/** The grid: 4:3, small enough that a frame is a few KB. */
export const GRID_W = 160;
export const GRID_H = 120;
/** Below this much of the edge map changing, a frame is the same frame. */
const STILL = 0.04;
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
  /** Whether it was sent, or held back as still. */
  sent: boolean;
}

export interface Camera {
  /** Frames a second. */
  setRate(fps: number): void;
  setDetail(d: number): void;
  stop(): void;
}

export interface CameraOptions {
  fps: number;
  detail: number;
  /** Every frame traced (sent or not), for your own tile. */
  onFrame(f: Frame, s: FrameStats): void;
  /** A frame worth sending, as wire bytes. */
  send(bytes: Uint8Array): void;
  onError(e: Error): void;
}

export async function startCamera(o: CameraOptions): Promise<Camera> {
  const stream = await navigator.mediaDevices.getUserMedia({
    video: { width: { ideal: 320 }, height: { ideal: 240 }, frameRate: { ideal: 15 } },
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
      g.drawImage(video, (vw - sw) / 2, (vh - sh) / 2, sw, sh, 0, 0, GRID_W, GRID_H);
      const t0 = performance.now();
      const img = g.getImageData(0, 0, GRID_W, GRID_H);
      const t = trace(img.data, GRID_W, GRID_H, { detail, prev });
      prev = t.edges;
      const frame = { w: t.w, h: t.h, lines: t.lines };
      const raw = pack(frame).length;
      const now = Date.now();
      const send = t.changed >= STILL || now - lastSent >= KEEP;
      const wire = send ? await encodeFrame(frame) : null;
      const ms = performance.now() - t0;
      const points = t.lines.reduce((s, l) => s + l.length / 2, 0);
      o.onFrame(frame, { bytes: wire?.length ?? 0, raw, lines: t.lines.length, points, ms, sent: send });
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

  return {
    setRate(f) {
      fps = f;
    },
    setDetail(d) {
      detail = d;
    },
    stop() {
      stopped = true;
      if (timer !== undefined) clearTimeout(timer);
      for (const t of stream.getTracks()) t.stop();
      video.srcObject = null;
    },
  };
}
