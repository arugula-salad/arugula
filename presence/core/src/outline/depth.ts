// How far away each part of a camera frame is (S43), from the frame alone:
// Depth Anything V2 Small (4-bit) through transformers.js on WebGPU, in
// this page. Only a near-to-far value per grid cell leaves this module.
//
// A room barely moves, so depth runs in the background a couple of times a
// second and the frames between use its last answer; the edges and labels
// keep up with you at the camera's rate. No WebGPU, no depth: on the CPU
// it's seconds a frame.
//
// The model and ONNX Runtime's Wasm are the page's own files (../assets,
// fetched by scripts/assets.sh); transformers.js would otherwise fetch
// them from Hugging Face and jsDelivr, and its Wasm cache makes a blob:
// script, which a strict CSP (Arugula control's) refuses.

import { env, pipeline, RawImage } from "@huggingface/transformers";
import { assets } from "../assets.ts";


export interface Depth {
  /** `src`'s depth on a `w`×`h` grid: 255 nearest, 0 farthest (relative
   * to this frame). */
  estimate(src: HTMLCanvasElement, w: number, h: number): Promise<Uint8Array>;
  close(): void;
}

export async function startDepth(): Promise<Depth> {
  const base = `${assets.base}depth/`;
  if (!("gpu" in navigator) || !(await (navigator as Navigator & { gpu: { requestAdapter(): Promise<unknown> } }).gpu.requestAdapter())) {
    throw new Error("this browser has no WebGPU");
  }
  env.allowRemoteModels = false;
  env.allowLocalModels = true;
  env.localModelPath = `${base}models/`;
  env.useWasmCache = false;
  const wasm = env.backends.onnx.wasm;
  if (wasm) wasm.wasmPaths = { mjs: `${base}ort-wasm-simd-threaded.asyncify.mjs`, wasm: `${base}ort-wasm-simd-threaded.asyncify.wasm` };
  const est = await pipeline("depth-estimation", "depth-anything-v2-small", { device: "webgpu", dtype: "q4f16" });
  return {
    async estimate(src, w, h) {
      const out = (await est(RawImage.fromCanvas(src))) as { depth: RawImage } | { depth: RawImage }[];
      const d = (Array.isArray(out) ? out[0] : out).depth;
      const grid = new Uint8Array(w * h);
      const ch = d.channels;
      for (let y = 0; y < h; y++) {
        const dy = Math.min(d.height - 1, Math.floor(((y + 0.5) / h) * d.height));
        for (let x = 0; x < w; x++) {
          const dx = Math.min(d.width - 1, Math.floor(((x + 0.5) / w) * d.width));
          grid[y * w + x] = d.data[(dy * d.width + dx) * ch];
        }
      }
      return grid;
    },
    close() {
      void est.dispose();
    },
  };
}
