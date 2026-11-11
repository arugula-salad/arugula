// A shared Doc's snapshot (#1066), opened. The daemon makes it
// (`crates/arugulad-docs/src/docs/link.rs` says the format): "ADL1", a big-endian
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

/** The proto's `DocLayout`, written out: this file imports nothing. */
export type DocLayout = "doc" | "tabs" | "prototype";

export interface Manifest {
  title: string;
  /** Absent in a snapshot from before layouts, or of a plain Doc; read it
   * with `layoutOf`. */
  layout?: string;
  /** When the Doc was made (absent where it isn't known: a Doc from before
   * Docs were kept) and last changed, in ms; absent in a snapshot from
   * before them (Arugula #825). */
  made?: number;
  updated?: number;
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

/** The key in a URL's fragment (`#k=…`, and a tab after it as `&s=s2`): 32 bytes, or null. */
export function keyFromHash(hash: string): Uint8Array | null {
  const m = /^#?k=([A-Za-z0-9_-]+)(?:&s=[\w-]+)?$/.exec(hash);
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

/** The snapshot's layout; a missing one, or one a newer machine made that
 * this build doesn't know, is `doc`. */
export function layoutOf(m: Manifest): DocLayout {
  return m.layout === "tabs" || m.layout === "prototype" ? m.layout : "doc";
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

// ------------------------------------------------------------------ sealing
//
// A team Doc's public link is sealed in the browser that asks (Arugula #871),
// with the format `link.rs` makes: the same snapshot, the same cut into parts,
// the same additional data. The platform holds the key; control only ever
// gets what `sealParts` returns. `link-vector.json` is made in Rust and
// `e2e/doc-link.spec.ts` checks these bytes against it, with the nonces fixed.

/** What a part adds to its plaintext: the nonce and the tag. */
const OVERHEAD = 12 + 16;
/** The most plaintext in one part, so that a sealed one is at most 1 MiB. */
export const PART_PLAIN = (1 << 20) - OVERHEAD;

/** The snapshot's plaintext: "ADL1", the manifest's length, the manifest as
 * JSON in the order Rust writes it, then each asset's bytes in order. */
export function snapshotBytes(m: Manifest, blobs: Uint8Array[]): Uint8Array {
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

/** `plain` cut into parts of at most `partMax` and each sealed on its own as
 * `nonce(12) || AES-256-GCM(ciphertext, tag)` under `key`, bound to the
 * link, version, place and count. At least one part, even for nothing.
 * `nonce` is for the spec: a fresh random one for every part otherwise. */
export async function sealParts(
  key: Uint8Array,
  id: string,
  ver: number,
  plain: Uint8Array,
  partMax: number = PART_PLAIN,
  nonce: (n: number) => Uint8Array = () => crypto.getRandomValues(new Uint8Array(12)),
): Promise<Uint8Array[]> {
  if (key.length !== 32) throw new Broken("a link's key is 32 bytes");
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

export function toBase64Url(b: Uint8Array): string {
  return toBase64(b).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
