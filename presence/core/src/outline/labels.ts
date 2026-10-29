// What a camera frame's lines trace (S43): you, your chair, your screen.
// Two MediaPipe models, run in this page on the same cropped frame the
// edges come from: DeepLab v3 labels every pixel with one of Pascal VOC's
// 21 classes, and the face landmarker gives your eyes, brows and lips as
// lines of their own. Only the labels leave this module, as a class per
// grid cell and face polylines in grid cells; the frame stays here.
//
// The models and MediaPipe's Wasm are the page's own files (../assets,
// fetched by scripts/assets.sh), loaded only when someone turns their
// outline on; this module is imported only then too.

import { FaceLandmarker, ImageSegmenter } from "@mediapipe/tasks-vision";
import type { Polyline } from "./encode.ts";
import { assets } from "../assets.ts";

type Connection = { start: number; end: number };

export interface Labels {
  /** A class per grid cell, row by row. */
  mask: Uint8Array;
  /** Eyes, brows, irises and lips, in grid cells. */
  face: Polyline[];
}

export interface Labeller {
  /** `src` is the frame (any size), labelled onto a `w`×`h` grid. */
  label(src: HTMLCanvasElement, w: number, h: number): Labels;
  close(): void;
}

export async function startLabeller(): Promise<Labeller> {
  const base = `${assets.base}mediapipe/`;
  const files = { wasmLoaderPath: `${base}vision_wasm_internal.js`, wasmBinaryPath: `${base}vision_wasm_internal.wasm` };
  const make = async (delegate: "GPU" | "CPU") =>
    Promise.all([
      ImageSegmenter.createFromOptions(files, {
        baseOptions: { modelAssetPath: `${base}deeplab_v3.tflite`, delegate },
        runningMode: "VIDEO",
        outputCategoryMask: true,
        outputConfidenceMasks: false,
      }),
      FaceLandmarker.createFromOptions(files, {
        baseOptions: { modelAssetPath: `${base}face_landmarker.task`, delegate },
        runningMode: "VIDEO",
        numFaces: 2,
      }),
    ]);
  // The GPU where there is one (WebGL); the CPU where it won't start.
  const [seg, face] = await make("GPU").catch(() => make("CPU"));
  const parts = [
    FaceLandmarker.FACE_LANDMARKS_LEFT_EYE,
    FaceLandmarker.FACE_LANDMARKS_RIGHT_EYE,
    FaceLandmarker.FACE_LANDMARKS_LEFT_EYEBROW,
    FaceLandmarker.FACE_LANDMARKS_RIGHT_EYEBROW,
    FaceLandmarker.FACE_LANDMARKS_LEFT_IRIS,
    FaceLandmarker.FACE_LANDMARKS_RIGHT_IRIS,
    FaceLandmarker.FACE_LANDMARKS_LIPS,
  ].map(chained);
  let last = 0;
  return {
    label(src, w, h) {
      // VIDEO mode wants a timestamp that only goes up.
      const now = Math.max(performance.now(), last + 1);
      last = now;
      const mask = new Uint8Array(w * h);
      const r = seg.segmentForVideo(src, now);
      const m = r.categoryMask;
      if (m) {
        const cats = m.getAsUint8Array();
        const mw = m.width, mh = m.height;
        for (let y = 0; y < h; y++) {
          const my = Math.min(mh - 1, Math.floor(((y + 0.5) / h) * mh));
          for (let x = 0; x < w; x++) mask[y * w + x] = cats[my * mw + Math.min(mw - 1, Math.floor(((x + 0.5) / w) * mw))];
        }
      }
      r.close();
      const lines: Polyline[] = [];
      for (const pts of face.detectForVideo(src, now).faceLandmarks) {
        for (const part of parts) {
          for (const run of part) {
            const l: Polyline = [];
            for (const i of run) {
              const p = pts[i];
              if (p) l.push(Math.round(Math.min(w - 1, Math.max(0, p.x * w))), Math.round(Math.min(h - 1, Math.max(0, p.y * h))));
            }
            if (l.length >= 4) lines.push(l);
          }
        }
      }
      return { mask, face: lines };
    },
    close() {
      seg.close();
      face.close();
    },
  };
}

/** A part's connections (pairs of landmarks) as runs of landmarks: joined
 * where one ends and the next starts, so an eye is one line, not sixteen. */
function chained(cs: Connection[]): number[][] {
  const runs: number[][] = [];
  for (const c of cs) {
    const run = runs.find((r) => r[r.length - 1] === c.start);
    if (run) run.push(c.end);
    else runs.push([c.start, c.end]);
  }
  // Join runs whose end is another's start.
  for (let joined = true; joined; ) {
    joined = false;
    for (const a of runs) {
      const b = runs.find((r) => r !== a && r[0] === a[a.length - 1]);
      if (b) {
        a.push(...b.slice(1));
        runs.splice(runs.indexOf(b), 1);
        joined = true;
        break;
      }
    }
  }
  return runs;
}
