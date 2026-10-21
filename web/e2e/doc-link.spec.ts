// #1066: a shared Doc's snapshot, opened by the web's own code. The daemon
// makes `crates/daemon/tests/fixtures/docs/link-vector.json` (a Rust test
// checks it byte for byte against the code that seals), and these open it
// with `src/doc-link.ts`, as the page does, on WebCrypto alone: no browser.
// Also what the share page may pull in, read from the build.

import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import { Broken, additionalData, fromBase64Url, keyFromHash, openParts, openSnapshot } from "../src/shell/doc-link";

const here = dirname(fileURLToPath(import.meta.url));
const vector = JSON.parse(readFileSync(join(here, "../../crates/daemon/tests/fixtures/docs/link-vector.json"), "utf8")) as {
  id: string;
  ver: number;
  key: string;
  parts: string[];
  manifest: { title: string; entries: { id: string; heading: string; intent: string; body: string | null }[]; assets: { name: string; mime: string; size: number }[] };
  assets: { name: string; b64: string }[];
};

const bytes = (b64: string) => new Uint8Array(Buffer.from(b64, "base64"));
const key = fromBase64Url(vector.key);
const parts = () => vector.parts.map(bytes);

test("the vector made in Rust opens in the web's code", async () => {
  const { manifest, blobs } = await openSnapshot(key, vector.id, vector.ver, parts());
  expect(manifest).toEqual(vector.manifest);
  expect(blobs.map((b) => Buffer.from(b).toString("base64"))).toEqual(vector.assets.map((a) => a.b64));
  expect(vector.parts.length).toBeGreaterThanOrEqual(4);
  // The additional data is the bytes the daemon binds.
  expect(new TextDecoder().decode(additionalData(vector.id, vector.ver, 2, 5))).toBe(`ADL1|${vector.id}|${vector.ver}|2|5`);
});

test("the key is read from the fragment, 32 bytes of base64url, and nothing else", () => {
  expect(keyFromHash(`#k=${vector.key}`)).toEqual(key);
  expect(keyFromHash(`k=${vector.key}`)).toEqual(key);
  for (const bad of ["", "#", "#k=", "#k=AAAA", `#k=${vector.key}x`, `#k=${vector.key}=`, `#k=${vector.key.replace(/.$/, "!")}`, `#key=${vector.key}`, `#k=${vector.key}&x=1`]) {
    expect(keyFromHash(bad), bad).toBeNull();
  }
});

test("swapped parts, parts of another version, another link, a dropped end or a wrong key don't open", async () => {
  const open = (k = key, id = vector.id, ver = vector.ver, p = parts()) => openSnapshot(k, id, ver, p);
  await open();

  const swapped = parts();
  [swapped[1], swapped[2]] = [swapped[2]!, swapped[1]!];
  await expect(open(key, vector.id, vector.ver, swapped)).rejects.toBeInstanceOf(Broken);
  // The decryption itself refuses, not just the snapshot's checks after it.
  await expect(openParts(key, vector.id, vector.ver, swapped)).rejects.toBeInstanceOf(Broken);

  // Part 0 of another version: sealed by the same key, bound to ver+1.
  await expect(open(key, vector.id, vector.ver + 1, parts())).rejects.toBeInstanceOf(Broken);
  const other = await sealOne(key, vector.id, vector.ver + 1, 0, parts().length, new Uint8Array(40));
  const mixed = parts();
  mixed[0] = other;
  await expect(open(key, vector.id, vector.ver, mixed)).rejects.toBeInstanceOf(Broken);

  await expect(open(key, "f".repeat(32))).rejects.toBeInstanceOf(Broken);
  await expect(open(key, vector.id, vector.ver, parts().slice(0, -1))).rejects.toBeInstanceOf(Broken);
  await expect(open(key, vector.id, vector.ver, [...parts(), parts()[0]!])).rejects.toBeInstanceOf(Broken);
  await expect(open(key, vector.id, vector.ver, [])).rejects.toBeInstanceOf(Broken);

  const wrong = key.slice();
  wrong[0]! ^= 1;
  await expect(open(wrong)).rejects.toBeInstanceOf(Broken);
  await expect(open(key.slice(0, 16))).rejects.toBeInstanceOf(Broken);

  const flipped = parts();
  flipped[3]![20]! ^= 1;
  await expect(open(key, vector.id, vector.ver, flipped)).rejects.toBeInstanceOf(Broken);
});

/** Seal `plain` as part `n` of `of` under `key`, as the daemon does. */
async function sealOne(k: Uint8Array, id: string, ver: number, n: number, of: number, plain: Uint8Array): Promise<Uint8Array> {
  const iv = new Uint8Array(12).fill(9);
  const ck = await crypto.subtle.importKey("raw", k as BufferSource, "AES-GCM", false, ["encrypt"]);
  const ct = new Uint8Array(await crypto.subtle.encrypt({ name: "AES-GCM", iv, additionalData: additionalData(id, ver, n, of) as BufferSource, tagLength: 128 }, ck, plain as BufferSource));
  return new Uint8Array([...iv, ...ct]);
}

test("a snapshot with a bad magic, manifest or asset length is broken, though its parts open", async () => {
  const seal = (plain: Uint8Array) => sealOne(key, vector.id, 1, 0, 1, plain).then((p) => [p]);
  const manifest = new TextEncoder().encode(JSON.stringify({ title: "T", entries: [], assets: [] }));
  const frame = (magic: string, len: number, body: Uint8Array, tail = new Uint8Array(0)) => {
    const out = new Uint8Array(8 + body.length + tail.length);
    out.set(new TextEncoder().encode(magic));
    new DataView(out.buffer).setUint32(4, len);
    out.set(body, 8);
    out.set(tail, 8 + body.length);
    return out;
  };
  const open = async (plain: Uint8Array) => openSnapshot(key, vector.id, 1, await seal(plain));
  expect((await open(frame("ADL1", manifest.length, manifest))).manifest.title).toBe("T");
  await expect(open(frame("ADL2", manifest.length, manifest))).rejects.toBeInstanceOf(Broken);
  await expect(open(frame("ADL1", manifest.length + 5, manifest))).rejects.toBeInstanceOf(Broken);
  await expect(open(frame("ADL1", manifest.length, manifest, new Uint8Array(3)))).rejects.toBeInstanceOf(Broken);
  const notJson = new TextEncoder().encode("{nope");
  await expect(open(frame("ADL1", notJson.length, notJson))).rejects.toBeInstanceOf(Broken);
  const shape = new TextEncoder().encode(JSON.stringify({ title: 3, entries: [], assets: [] }));
  await expect(open(frame("ADL1", shape.length, shape))).rejects.toBeInstanceOf(Broken);
  const cut = new TextEncoder().encode(JSON.stringify({ title: "T", entries: [], assets: [{ name: "a.png", mime: "image/png", size: 10 }] }));
  await expect(open(frame("ADL1", cut.length, cut, new Uint8Array(4)))).rejects.toBeInstanceOf(Broken);
  await expect(openParts(key, vector.id, 1, [new Uint8Array(10)])).rejects.toBeInstanceOf(Broken);
});

// The page behind a link must not pull in the app (#1064, decision 5): no
// client, telemetry, service worker or socket. Read from the build: the
// chunks `dist/doc.html` loads, and every chunk those import, by name and by
// what they contain.
test("the share page's chunks are the Doc renderer and nothing of the app", async () => {
  const dist = join(here, "../dist");
  test.skip(!existsSync(join(dist, "doc.html")), "needs the web build (just web)");
  const html = readFileSync(join(dist, "doc.html"), "utf8");
  const direct = [...html.matchAll(/(?:src|href)="\/(assets\/[^"]+\.js)"/g)].map((m) => m[1]!);
  const seen = new Set<string>();
  const walk = (f: string) => {
    if (seen.has(f)) return;
    seen.add(f);
    const js = readFileSync(join(dist, f), "utf8");
    for (const m of js.matchAll(/(?:from|import)\s*["']\.\/([^"']+\.js)["']/g)) walk(`assets/${m[1]}`);
    expect(js, f).not.toMatch(/WebSocket|serviceWorker|sendBeacon|XMLHttpRequest|EventSource|localStorage|import\(/);
    expect(js, f).not.toContain("/api/");
  };
  direct.forEach(walk);
  const names = [...seen].map((f) => f.replace(/^assets\//, "").replace(/-[\w-]{8}\.js$/, ""));
  console.log(`doc.html loads: ${[...seen].join(", ")}`);
  // The entry and the renderer (and the modulepreload polyfill Vite adds).
  expect(names.sort()).toEqual(["doc", "doc-render", "modulepreload-polyfill"]);
  // Styles only: the page's own, with the app's stylesheet inside it (a
  // chunk of its own would be shared with the app and reorder its cascade).
  expect([...html.matchAll(/rel="stylesheet"[^>]*href="([^"]+)"/g)].map((m) => m[1]!.replace(/-[\w-]{8}\.css$/, ""))).toEqual(["/assets/doc"]);
  const index = readFileSync(join(dist, "index.html"), "utf8");
  expect(index, "the app's own CSS stays the app's").not.toMatch(/assets\/style-/);
  // No icon request, no beacon, no other page's script.
  expect(html).toContain('<link rel="icon" href="data:,"');
  expect(html).toContain('<meta name="referrer" content="no-referrer"');
  expect(html.match(/<script/g)).toHaveLength(1);
});
