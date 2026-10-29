// What a line traces (S43), as a frame's class byte says it, and the
// colour each draws in. Kept apart from ./labels so a page that only
// draws outlines never loads MediaPipe.

/** DeepLab's classes (Pascal VOC): a line's class is its index. */
export const VOC = [
  "background", "aeroplane", "bicycle", "bird", "boat", "bottle", "bus", "car", "cat", "chair", "cow",
  "diningtable", "dog", "horse", "motorbike", "person", "pottedplant", "sheep", "sofa", "train", "tv",
] as const;
/** A line the face landmarker drew (not a VOC class). */
export const FACE = 32;

/** The groups a tile colours, each a theme colour, in the legend's order. */
export const GROUPS = [
  { name: "you", css: "--accent", classes: [15] },
  { name: "face", css: "--fg", classes: [FACE] },
  { name: "chair", css: "--warn", classes: [9, 18] },
  { name: "screen", css: "--info", classes: [20] },
  { name: "plant", css: "--ok", classes: [16] },
  { name: "pets", css: "--danger", classes: [8, 12] },
  { name: "things", css: "--teal", classes: [5, 11] },
] as const;

/** Where a class goes: its group's index, or -1 for the rest (dim). */
export function groupOf(cls: number): number {
  return GROUPS.findIndex((g) => (g.classes as readonly number[]).includes(cls));
}
