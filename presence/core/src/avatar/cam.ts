// Your camera as rig frames (S44): video only, tracked (./track, loaded
// now) at a steady rate, each rig packed (./rig) with your look and sent.
// The video stays in this page. A hidden page sends nothing.

import { packRig, type Look, type Rig } from "./rig.ts";
import type { Tracker } from "./track.ts";

export interface AvatarCamera {
  setRate(fps: number): void;
  setLook(l: Look): void;
  stop(): void;
}

export interface AvatarCameraOptions {
  fps: number;
  look: Look;
  /** Every rig, for your own tile, with how long tracking took. */
  onRig(r: Rig, ms: number): void;
  /** Wire bytes: S43's frame encoding byte (0, raw), then the rig. */
  send(bytes: Uint8Array): void;
  onError(e: Error): void;
}

export async function startAvatarCamera(o: AvatarCameraOptions): Promise<AvatarCamera> {
  const [stream, tracker] = await Promise.all([
    navigator.mediaDevices.getUserMedia({ video: { width: { ideal: 640 }, height: { ideal: 480 }, frameRate: { ideal: 30 } }, audio: false }),
    import("./track.ts").then((m) => m.startTracker()) as Promise<Tracker>,
  ]);
  const video = document.createElement("video");
  video.muted = true;
  video.playsInline = true;
  video.srcObject = stream;
  await video.play();
  let fps = o.fps;
  let look = o.look;
  let timer: number | undefined;
  let stopped = false;
  const tick = () => {
    timer = undefined;
    if (stopped) return;
    if (!document.hidden && video.videoWidth) {
      try {
        const t0 = performance.now();
        const rig = tracker.track(video);
        const ms = performance.now() - t0;
        o.onRig(rig, ms);
        const body = packRig(rig, look);
        const wire = new Uint8Array(body.length + 1);
        wire.set(body, 1);
        o.send(wire);
      } catch (e) {
        o.onError(e as Error);
      }
    }
    timer = window.setTimeout(tick, document.hidden ? 1000 : 1000 / fps);
  };
  tick();
  return {
    setRate(f) {
      fps = f;
    },
    setLook(l) {
      look = l;
    },
    stop() {
      stopped = true;
      if (timer !== undefined) clearTimeout(timer);
      tracker.close();
      for (const t of stream.getTracks()) t.stop();
      video.srcObject = null;
    },
  };
}
