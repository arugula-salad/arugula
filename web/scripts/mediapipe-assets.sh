#!/bin/sh
# S43: what the People block's labelling needs, served from control itself
# (its CSP takes nothing from elsewhere): MediaPipe's vision Wasm from
# node_modules, and two models from Google's MediaPipe model bucket
# (Apache-2.0): DeepLab v3 (21 Pascal VOC classes: person, chair, sofa,
# tv…) and the face landmarker. Into public/mediapipe/, which git ignores.
# A page fetches them only when someone turns their outline on.
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
