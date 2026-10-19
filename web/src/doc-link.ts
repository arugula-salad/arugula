// A shared Doc's snapshot (#1066), opened. The daemon makes it
// (`crates/daemon/src/docs/link.rs` says the format): "ADL1", a big-endian
// u32 length, the manifest as JSON, then the assets' bytes in the
// manifest's order, cut into parts that are each `nonce(12) || AES-256-GCM`
// with `ADL1|<id>|<ver>|<n>|<parts>` as additional data.
//
// This file imports nothing: the viewer page (doc-view.tsx) and a spec run
// it as it is, on WebCrypto alone.

/** Anything wrong with a link that isn't "it isn't shared": a missing key,
 * a part that doesn't open, a snapshot that isn't one. The page says only
 * that the link is broken. */
export class Broken extends Error {}

export interface ManifestEntry {
  id: string;
  heading: string;
  intent: string;
  body: string | null;
}

export interface ManifestAsset {
  name: string;
  mime: string;
  size: number;
}

export interface Manifest {
  title: string;
  entries: ManifestEntry[];
  assets: ManifestAsset[];
}

export interface Opened {
  manifest: Manifest;
  /** Each asset's bytes, in the manifest's order. */
  blobs: Uint8Array[];
}

const MAGIC = [0x41, 0x44, 0x4c, 0x31]; // "ADL1"

export function fromBase64Url(s: string): Uint8Array {
  if (!/^[A-Za-z0-9_-]*$/.test(s)) throw new Broken("not base64url");
  const bin = atob(s.replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(bin, (c) => c.charCodeAt(0));
}

/** The key in a URL's fragment (`#k=…`): 32 bytes, or null. */
export function keyFromHash(hash: string): Uint8Array | null {
  const m = /^#?k=([A-Za-z0-9_-]+)$/.exec(hash);
  if (!m) return null;
  try {
    const key = fromBase64Url(m[1]!);
    return key.length === 32 ? key : null;
  } catch {
    return null;
  }
}

/** The additional data of part `n` of `parts`. */
export function additionalData(id: string, ver: number, n: number, parts: number): Uint8Array {
  return new TextEncoder().encode(`ADL1|${id}|${ver}|${n}|${parts}`);
}

/** The plaintext of the parts, or Broken if any one doesn't open under this
 * key, id, version, place and count. */
export async function openParts(key: Uint8Array, id: string, ver: number, parts: Uint8Array[]): Promise<Uint8Array> {
  if (key.length !== 32 || parts.length === 0) throw new Broken("no key or no parts");
  const k = await crypto.subtle.importKey("raw", key as BufferSource, "AES-GCM", false, ["decrypt"]);
  const plain: Uint8Array[] = [];
  for (let n = 0; n < parts.length; n++) {
    const part = parts[n]!;
    if (part.length < 12 + 16) throw new Broken("a part is too short");
    try {
      const out = await crypto.subtle.decrypt(
        { name: "AES-GCM", iv: part.subarray(0, 12) as BufferSource, additionalData: additionalData(id, ver, n, parts.length) as BufferSource, tagLength: 128 },
        k,
        part.subarray(12) as BufferSource,
      );
      plain.push(new Uint8Array(out));
    } catch {
      throw new Broken("a part doesn't open");
    }
  }
  const all = new Uint8Array(plain.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const p of plain) {
    all.set(p, at);
    at += p.length;
  }
  return all;
}

const str = (v: unknown): v is string => typeof v === "string";

function manifestOf(v: unknown): Manifest {
  const m = v as Partial<Manifest> | null;
  if (!m || !str(m.title) || !Array.isArray(m.entries) || !Array.isArray(m.assets)) throw new Broken("not a manifest");
  for (const e of m.entries) {
    if (!e || !str(e.id) || !str(e.heading) || !str(e.intent) || !(e.body === null || str(e.body))) throw new Broken("a bad section");
  }
  for (const a of m.assets) {
    if (!a || !str(a.name) || !str(a.mime) || !Number.isSafeInteger(a.size) || a.size < 0) throw new Broken("a bad asset");
  }
  return m as Manifest;
}

/** The snapshot in these parts. */
export async function openSnapshot(key: Uint8Array, id: string, ver: number, parts: Uint8Array[]): Promise<Opened> {
  const plain = await openParts(key, id, ver, parts);
  if (plain.length < 8 || MAGIC.some((b, i) => plain[i] !== b)) throw new Broken("bad magic");
  const len = new DataView(plain.buffer, plain.byteOffset, plain.byteLength).getUint32(4);
  if (len > plain.length - 8) throw new Broken("bad manifest length");
  let manifest: Manifest;
  try {
    manifest = manifestOf(JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(plain.subarray(8, 8 + len))));
  } catch (e) {
    throw e instanceof Broken ? e : new Broken("bad manifest");
  }
  let at = 8 + len;
  const blobs: Uint8Array[] = [];
  for (const a of manifest.assets) {
    if (at + a.size > plain.length) throw new Broken("an asset is cut short");
    blobs.push(plain.subarray(at, at + a.size));
    at += a.size;
  }
  if (at !== plain.length) throw new Broken("bytes after the last asset");
  return { manifest, blobs };
}

export function toBase64(b: Uint8Array): string {
  let s = "";
  for (const x of b) s += String.fromCharCode(x);
  return btoa(s);
}
