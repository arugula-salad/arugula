#!/bin/sh
# S43: what the People block's labelling and depth need, served from
# control itself (its CSP takes nothing from elsewhere), into public/,
# which git ignores for these. A page fetches them only when someone turns
# their outline on.
#   public/mediapipe/  MediaPipe's vision Wasm (node_modules) and two models
#                      from Google's MediaPipe bucket (Apache-2.0): DeepLab
#                      v3 (21 Pascal VOC classes) and the face landmarker.
#   public/depth/      ONNX Runtime Web's asyncify build (node_modules, what
#                      transformers.js runs WebGPU on) and Depth Anything V2
#                      Small at 4 bits (Apache-2.0) from Hugging Face, its
#                      input cut from 518 px to 266 so it runs in tens of ms.
set -eu
cd "$(dirname "$0")/.."
out=public/mediapipe
mkdir -p "$out"
wasm=node_modules/@mediapipe/tasks-vision/wasm
cp "$wasm/vision_wasm_internal.js" "$wasm/vision_wasm_internal.wasm" "$out/"
get() {
  [ -s "$out/$1" ] || curl -fsSL --retry 2 -o "$out/$1" "$2"
}
get deeplab_v3.tflite https://storage.googleapis.com/mediapipe-models/image_segmenter/deeplab_v3/float32/1/deeplab_v3.tflite
get face_landmarker.task https://storage.googleapis.com/mediapipe-models/face_landmarker/face_landmarker/float16/1/face_landmarker.task
ls -l "$out"

depth=public/depth
model=$depth/models/depth-anything-v2-small
mkdir -p "$model/onnx"
ort=$(realpath node_modules/@huggingface/transformers)/../../onnxruntime-web/dist
cp "$ort/ort-wasm-simd-threaded.asyncify.mjs" "$ort/ort-wasm-simd-threaded.asyncify.wasm" "$depth/"
hf=https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/main
[ -s "$model/config.json" ] || curl -fsSL --retry 2 -o "$model/config.json" "$hf/config.json"
[ -s "$model/onnx/model_q4f16.onnx" ] || curl -fsSL --retry 2 -o "$model/onnx/model_q4f16.onnx" "$hf/onnx/model_q4f16.onnx"
[ -s "$model/preprocessor_config.json" ] || curl -fsSL --retry 2 "$hf/preprocessor_config.json" |
  node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const c=JSON.parse(s);c.size={height:266,width:266};process.stdout.write(JSON.stringify(c,null,2))})' >"$model/preprocessor_config.json"
ls -lR "$depth"
