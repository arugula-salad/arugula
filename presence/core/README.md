# @arugula/presence-core

A camera as who's around, without a picture leaving the page (Arugula S43,
S44; arugula.video, Arugula #315). Two kinds of frame, both a few bytes to a
few KB:

- **Outlines** (`src/outline/`): the camera traced into line art
  (`encode.ts`), packed (`frame.ts`), drawn (`render.ts`); optionally each
  line coloured by what it traces (`labels.ts`: MediaPipe DeepLab and the
  face landmarker) and how near it is (`depth.ts`: Depth Anything V2 on
  WebGPU). `camera.ts` runs the loop.
- **Avatars** (`src/avatar/`): the camera as a rig, your pose as numbers
  (`rig.ts`, from MediaPipe's face, pose and hand landmarkers in
  `track.ts`, solved and smoothed in `solve.ts`), and the characters every
  page draws from it (`character.ts`'s contract, `characters/`, drawn by
  `stage.ts`). `cam.ts` runs the loop.

No framework and no network: `@arugula/presence` (`../client`) is the room
client. TypeScript source, no build step: a bundler (Vite) or `tsc`
consumes `src/` as it is. The models and Wasm load only when a camera
turns on, by dynamic import; `src/index.ts` has the light modules.

## Models and Wasm

`scripts/assets.sh <public dir>` copies MediaPipe's and ONNX Runtime's
Wasm out of `node_modules` and fetches the models (about 65 MB) into
`<dir>/mediapipe` and `<dir>/depth`, for the page to serve itself (a strict
CSP takes nothing from elsewhere). `setAssetBase()` (`src/assets.ts`)
points the page elsewhere. Arugula's web client runs it as
`web/scripts/outline-assets.sh`.

## Working on it

    pnpm install
    pnpm test        # node: the encoder, frames, rig, solver, smoothing
    pnpm typecheck
    pnpm sheet       # the contact sheet: every character in every pose, at http://127.0.0.1:5199/

The contact sheet (`sheet/`) draws each character as a bust and as a 36 px
chat avatar through the real stage, for tuning characters by eye.
