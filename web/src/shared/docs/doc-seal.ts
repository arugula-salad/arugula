// Sealing a team Doc's public link in the browser (Arugula #871), in the
// format `crates/arugulad-docs/src/docs/link.rs` makes and the viewer
// (`shell/doc-link.ts`, which imports nothing and so can't be shared) opens:
// "ADL1", a big-endian u32 length, the manifest as JSON, then the assets'
// bytes, cut into parts that are each `nonce(12) || AES-256-GCM` with
// `ADL1|<id>|<ver>|<n>|<parts>` as additional data. The platform holds the
// key; control only ever gets what `sealParts` returns.
// `crates/daemon/tests/fixtures/docs/link-vector.json` is made in Rust and
// `e2e/doc-link.spec.ts` checks these bytes against it, with the nonces fixed.
// WebCrypto only.

export interface SealManifest {
  title: string;
  layout?: string;
  made?: number;
  updated?: number;
  entries: { id: string; heading: string; intent: string; body: string | null }[];
  assets: { name: string; mime: string; size: number }[];
}

const MAGIC = [0x41, 0x44, 0x4c, 0x31]; // "ADL1"
/** What a part adds to its plaintext: the nonce and the tag. */
const OVERHEAD = 12 + 16;
/** The most plaintext in one part, so that a sealed one is at most 1 MiB. */
export const PART_PLAIN = (1 << 20) - OVERHEAD;

export function fromBase64Url(s: string): Uint8Array {
  if (!/^[A-Za-z0-9_-]*$/.test(s)) throw new Error("not base64url");
  const bin = atob(s.replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

export function toBase64Url(b: Uint8Array): string {
  let s = "";
  for (const x of b) s += String.fromCharCode(x);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

/** The additional data of part `n` of `parts`. */
function additionalData(id: string, ver: number, n: number, parts: number): Uint8Array {
  return new TextEncoder().encode(`ADL1|${id}|${ver}|${n}|${parts}`);
}

/** The snapshot's plaintext: "ADL1", the manifest's length, the manifest as
 * JSON in the order Rust writes it, then each asset's bytes in order. */
export function snapshotBytes(m: SealManifest, blobs: Uint8Array[]): Uint8Array {
  const ordered: Record<string, unknown> = { title: m.title };
  if (m.layout && m.layout !== "doc") ordered.layout = m.layout;
  if (m.made !== undefined) ordered.made = m.made;
  if (m.updated !== undefined) ordered.updated = m.updated;
  ordered.entries = m.entries.map((e) => ({ id: e.id, heading: e.heading, intent: e.intent, body: e.body }));
  ordered.assets = m.assets.map((a) => ({ name: a.name, mime: a.mime, size: a.size }));
  const json = new TextEncoder().encode(JSON.stringify(ordered));
  const out = new Uint8Array(8 + json.length + blobs.reduce((n, b) => n + b.length, 0));
  out.set(MAGIC);
  new DataView(out.buffer).setUint32(4, json.length);
  out.set(json, 8);
  let at = 8 + json.length;
  for (const b of blobs) {
    out.set(b, at);
    at += b.length;
  }
  return out;
}

/** `plain` cut into parts of at most `partMax` and each sealed on its own
 * under `key`, bound to the link, version, place and count. At least one
 * part, even for nothing. `nonce` is for the spec: a fresh random one for
 * every part otherwise. */
export async function sealParts(
  key: Uint8Array,
  id: string,
  ver: number,
  plain: Uint8Array,
  partMax: number = PART_PLAIN,
  nonce: (n: number) => Uint8Array = () => crypto.getRandomValues(new Uint8Array(12)),
): Promise<Uint8Array[]> {
  if (key.length !== 32) throw new Error("a link's key is 32 bytes");
  const k = await crypto.subtle.importKey("raw", key as BufferSource, "AES-GCM", false, ["encrypt"]);
  const total = Math.max(1, Math.ceil(plain.length / partMax));
  const parts: Uint8Array[] = [];
  for (let n = 0; n < total; n++) {
    const iv = nonce(n);
    const chunk = plain.subarray(n * partMax, (n + 1) * partMax);
    const ct = new Uint8Array(
      await crypto.subtle.encrypt({ name: "AES-GCM", iv: iv as BufferSource, additionalData: additionalData(id, ver, n, total) as BufferSource, tagLength: 128 }, k, chunk as BufferSource),
    );
    const part = new Uint8Array(12 + ct.length);
    part.set(iv);
    part.set(ct, 12);
    parts.push(part);
  }
  return parts;
}
