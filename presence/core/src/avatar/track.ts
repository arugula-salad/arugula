// Your camera as a rig (S44): the face landmarker (with its blendshapes
// and head transform), the pose landmarker, lite, and the hand landmarker
// on each video frame, solved (./solve) and smoothed. Loaded only when
// someone picks the avatar and turns their camera on; the models are
// the page's own files (scripts/assets.sh, ../assets), beside S43's.

import { FaceLandmarker, HandLandmarker, PoseLandmarker } from "@mediapipe/tasks-vision";
import { RigFilter, solve, type Shapes } from "./solve.ts";
import type { Rig } from "./rig.ts";
import { assets } from "../assets.ts";


export interface Tracker {
  track(video: HTMLVideoElement): Rig;
  close(): void;
}

export async function startTracker(): Promise<Tracker> {
  const base = `${assets.base}mediapipe/`;
  const files = { wasmLoaderPath: `${base}vision_wasm_internal.js`, wasmBinaryPath: `${base}vision_wasm_internal.wasm` };
  const make = (delegate: "GPU" | "CPU") =>
    Promise.all([
      FaceLandmarker.createFromOptions(files, {
        baseOptions: { modelAssetPath: `${base}face_landmarker.task`, delegate },
        runningMode: "VIDEO",
        numFaces: 1,
        outputFaceBlendshapes: true,
        outputFacialTransformationMatrixes: true,
      }),
      PoseLandmarker.createFromOptions(files, {
        baseOptions: { modelAssetPath: `${base}pose_landmarker_lite.task`, delegate },
        runningMode: "VIDEO",
        numPoses: 1,
      }),
      HandLandmarker.createFromOptions(files, {
        baseOptions: { modelAssetPath: `${base}hand_landmarker.task`, delegate },
        runningMode: "VIDEO",
        numHands: 2,
      }),
    ]);
  const [face, pose, hands] = await make("GPU").catch(() => make("CPU"));
  const filter = new RigFilter();
  const calib = {};
  let last = 0;
  return {
    track(video) {
      const now = Math.max(performance.now(), last + 1);
      last = now;
      const f = face.detectForVideo(video, now);
      const p = pose.detectForVideo(video, now);
      const h = hands.detectForVideo(video, now);
      const shapes: Shapes | null = f.faceBlendshapes?.[0]
        ? Object.fromEntries(f.faceBlendshapes[0].categories.map((c) => [c.categoryName, c.score]))
        : null;
      const rig = solve({
        matrix: f.facialTransformationMatrixes?.[0]?.data ?? null,
        shapes,
        face: f.faceLandmarks?.[0] ?? null,
        pose: p.landmarks?.[0] ?? null,
        hands: h.landmarks ?? null,
        aspect: video.videoWidth / (video.videoHeight || 1) || 4 / 3,
        poseWorld: p.worldLandmarks?.[0] ?? null,
      }, calib);
      p.close();
      return filter.apply(rig, now / 1000);
    },
    close() {
      face.close();
      pose.close();
      hands.close();
    },
  };
}
