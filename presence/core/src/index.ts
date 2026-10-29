// @arugula/presence-core: a camera as who's around, without a picture
// leaving the page (Arugula S43, S44). Two kinds of frame: line outlines
// traced from the camera (./outline), or a rig, the pose as a few dozen
// bytes, that drives a character every page draws (./avatar).
//
// This index has the light modules only. The models load where they're
// used, by dynamic import: outline/labels and outline/depth from the
// outline camera, avatar/track from the avatar camera.

export { setAssetBase } from "./assets.ts";
export { trace, simplify, byDepth, type Polyline, type Traced, type TraceOptions } from "./outline/encode.ts";
export { encodeFrame, decodeFrame, pack, unpack, VERSION, type Frame } from "./outline/frame.ts";
export { draw, type DrawOptions } from "./outline/render.ts";
export { startCamera, GRID_W, GRID_H, type Camera, type CameraOptions, type FrameStats } from "./outline/camera.ts";
export { VOC, FACE, GROUPS, groupOf, palette } from "./outline/classes.ts";
export {
  RIG, packRig, unpackRig, neutral, idle, approach, wrap, elbowFor, withElbows, waving, WAVE_MS, SHOULDER_L, SHOULDER_R,
  type Rig, type Look, type Hand, type Point,
} from "./avatar/rig.ts";
export { solve, handOf, OneEuro, RigFilter, type Seen, type Mark, type Shapes, type Calib } from "./avatar/solve.ts";
export { startAvatarCamera, type AvatarCamera, type AvatarCameraOptions } from "./avatar/cam.ts";
export { HEAD_Y, HEAD_R, type Character, type Stage } from "./avatar/character.ts";
export { animate, COLOURS, type Source, type ViewOptions } from "./avatar/stage.ts";
export { drawHand, type HandStyle } from "./avatar/hand.ts";
export { CHARACTERS, characterOf } from "./avatar/characters/index.ts";
