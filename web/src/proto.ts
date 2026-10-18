// The wire types are generated from crates/proto (proto.gen.ts, `just
// proto-ts`). This file adds the code that goes with them. The HTTP API's
// types that aren't generated yet are declared where they're used (#387).

import type { Gate, PaneId, ThreadTarget } from "./proto.gen";

export type * from "./proto.gen";
export { CONV_MEMBERS_SEE } from "./proto.gen";

/** What names a gate among its block's: `member/op/gate`. */
export function gateKey(g: Gate): string {
  return `${g.member}/${g.op}/${g.gate}`;
}

/** What *Expire* does to a chant gate (#310), for its tooltip. */
export const EXPIRE_TITLE = "Turn it down: clear the pending gate without approving it, so the next run stops there again";

/** `pane-7` / `session-2` / `conv-c-…`: a thread's name in its API path. */
export function threadKey(t: ThreadTarget): string {
  return "pane" in t ? `pane-${t.pane}` : "session" in t ? `session-${t.session}` : `conv-${t.conv}`;
}

/** A thread's key, `pane-N`, `session-N` or `conv-c-…` (#855), as a target. */
export function parseThreadKey(key: string): ThreadTarget | null {
  const m = /^(?:(pane|session)-(\d+)|conv-(c-[0-9a-f]{32}))$/.exec(key);
  if (!m) return null;
  if (m[3]) return { conv: m[3] };
  return m[1] === "pane" ? { pane: Number(m[2]) } : { session: Number(m[2]) };
}

export const enum FrameKind {
  Output = 1,
  Snapshot = 2,
  Input = 3,
  /** A snapshot compressed with zstd (for an attach with `zstd`). */
  SnapshotZstd = 4,
}

export interface Frame {
  kind: FrameKind;
  pane: PaneId;
  offset: number;
  data: Uint8Array;
}

const HEADER_LEN = 13;

export function encodeFrame(kind: FrameKind, pane: PaneId, data: Uint8Array, offset = 0): Uint8Array<ArrayBuffer> {
  const out = new Uint8Array(HEADER_LEN + data.length);
  const view = new DataView(out.buffer);
  view.setUint8(0, kind);
  view.setUint32(1, pane);
  view.setBigUint64(5, BigInt(offset));
  out.set(data, HEADER_LEN);
  return out;
}

export function decodeFrame(buf: ArrayBuffer): Frame {
  if (buf.byteLength < HEADER_LEN) throw new Error(`frame too short: ${buf.byteLength}`);
  const view = new DataView(buf);
  return {
    kind: view.getUint8(0) as FrameKind,
    pane: view.getUint32(1),
    // Offsets stay far below 2^53 (8 PB of output), so Number is exact.
    offset: Number(view.getBigUint64(5)),
    data: new Uint8Array(buf, HEADER_LEN),
  };
}
