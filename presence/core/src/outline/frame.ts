// An outline frame's bytes (S43): what a page sends control and control
// sends the people who may see it, opaque to control.
//
//   frame   = encoding:u8 body          0 raw, 1 deflate-raw of raw
//   raw     = version:u8 (3) w:u8 h:u8 count:varint line*
//   line    = points:varint class:u8 near:u8 x0:u8 y0:u8 (dx:zigzag-varint dy:zigzag-varint)*
//
// A line's class says what it traces (./classes: a Pascal VOC class, or
// the face landmarker's); 0 is unknown. Its near is how close it is
// (./depth), 255 nearest, 255 when nobody measured. Version 1 had neither,
// version 2 no near.
//
// A grid is at most 255 cells a side. Deltas are small (a simplified
// edge moves a few cells a step), so most take a byte each.

import type { Polyline } from "./encode.ts";

export const VERSION = 3;
const RAW = 0;
const DEFLATE = 1;

export interface Frame {
  w: number;
  h: number;
  lines: Polyline[];
  /** Each line's class; none is all 0. */
  classes?: number[];
  /** Each line's nearness, 255 nearest; none is all 255. */
  depths?: number[];
}

/** `f` as raw bytes. */
export function pack(f: Frame): Uint8Array {
  const out: number[] = [VERSION, f.w, f.h];
  varint(out, f.lines.length);
  f.lines.forEach((l, n) => {
    varint(out, l.length / 2);
    out.push(f.classes?.[n] ?? 0, f.depths?.[n] ?? 255, l[0], l[1]);
    for (let i = 2; i < l.length; i += 2) {
      varint(out, zigzag(l[i] - l[i - 2]));
      varint(out, zigzag(l[i + 1] - l[i - 1]));
    }
  });
  return Uint8Array.from(out);
}

/** Raw bytes as a frame; throws on anything that isn't one. */
export function unpack(b: Uint8Array): Frame {
  let p = 0;
  const byte = () => {
    if (p >= b.length) throw new Error("outline frame: cut short");
    return b[p++];
  };
  const read = () => {
    let v = 0, s = 0, c;
    do {
      c = byte();
      v += (c & 0x7f) * 2 ** s;
      s += 7;
    } while (c & 0x80 && s < 35);
    return v;
  };
  const version = byte();
  if (version < 1 || version > 3) throw new Error("outline frame: not a version this page reads");
  const w = byte(), h = byte();
  const count = read();
  const lines: Polyline[] = [];
  const classes: number[] = [];
  const depths: number[] = [];
  for (let n = 0; n < count; n++) {
    const m = read();
    classes.push(version >= 2 ? byte() : 0);
    depths.push(version >= 3 ? byte() : 255);
    const l: Polyline = [byte(), byte()];
    for (let i = 1; i < m; i++) l.push(l[l.length - 2] + unzigzag(read()), l[l.length - 1] + unzigzag(read()));
    lines.push(l);
  }
  return { w, h, lines, classes, depths };
}

/** `f` as it goes on the wire: deflated when that's smaller. */
export async function encodeFrame(f: Frame): Promise<Uint8Array> {
  const raw = pack(f);
  const small = await squeeze(raw, new CompressionStream("deflate-raw"));
  const out = new Uint8Array(1 + Math.min(raw.length, small.length));
  if (small.length < raw.length) {
    out[0] = DEFLATE;
    out.set(small, 1);
  } else {
    out[0] = RAW;
    out.set(raw, 1);
  }
  return out;
}

/** A frame off the wire. */
export async function decodeFrame(b: Uint8Array): Promise<Frame> {
  if (b[0] === RAW) return unpack(b.subarray(1));
  if (b[0] === DEFLATE) return unpack(await squeeze(b.subarray(1), new DecompressionStream("deflate-raw")));
  throw new Error("outline frame: unknown encoding");
}

async function squeeze(b: Uint8Array, t: CompressionStream | DecompressionStream): Promise<Uint8Array> {
  const s = new Blob([b as BlobPart]).stream().pipeThrough(t);
  return new Uint8Array(await new Response(s).arrayBuffer());
}

function varint(out: number[], v: number) {
  while (v >= 0x80) {
    out.push((v & 0x7f) | 0x80);
    v = Math.floor(v / 128);
  }
  out.push(v);
}

const zigzag = (v: number) => (v < 0 ? -v * 2 - 1 : v * 2);
const unzigzag = (v: number) => (v & 1 ? -(v + 1) / 2 : v / 2);
