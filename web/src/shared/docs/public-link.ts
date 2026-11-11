// A team Doc's public link, made from its page or the Docs list (Arugula
// #871). The platform keeps the link's key with the Doc and gives it to the
// people who read the Doc; this browser seals the Doc's snapshot with it
// (`shell/doc-link.ts`, the format `link.rs` makes), puts the ciphertext to
// control, and tells the platform which version is served. Control never has
// the key: nothing here sends it anywhere but the platform's own routes.

import type { ProjectDocPublic, ProjectDocView } from "../proto";
import { platformFetch, refusalOf } from "../maintenance";
import { fromBase64Url, sealParts, snapshotBytes, type SealManifest } from "./doc-seal";
import { DocError, projectDocs } from "./store";

/** Where the public viewer is, from `/control.json`'s `doc_viewer` (null
 * where this control keeps no shared Docs). */
export async function docViewer(): Promise<string | null> {
  try {
    const res = await fetch("/control.json", { cache: "no-store" });
    if (!res.ok) return null;
    const j = (await res.json()) as { doc_viewer?: string | null };
    return j.doc_viewer ?? null;
  } catch {
    return null;
  }
}

/** The URL anyone can open: `<viewer>/<id>#k=<key>`. */
export const publicUrl = (viewer: string, p: ProjectDocPublic) => `${viewer}/${p.id}#k=${p.key}`;

/** Whether the link serves something, and the Doc has moved on since. */
export const publicBehind = (p: ProjectDocPublic | undefined) => !!p && p.ver > 0 && p.behind;

/** The Doc as a snapshot's manifest and picture bytes. */
async function snapshotOf(project: string, v: ProjectDocView): Promise<{ manifest: SealManifest; blobs: Uint8Array[] }> {
  const blobs = await Promise.all(v.assets.map((a) => projectDocs.assetBytes(project, v.doc.id, a.name)));
  const manifest: SealManifest = {
    title: v.doc.title,
    layout: v.doc.layout ?? "doc",
    made: v.doc.created_at,
    updated: v.doc.updated_at,
    entries: v.sections.map((s) => ({ id: s.id, heading: s.heading, intent: s.intent, body: s.body })),
    assets: v.assets.map((a, i) => ({ name: a.name, mime: a.mime, size: blobs[i]!.length })),
  };
  return { manifest, blobs };
}

async function control(method: string, path: string, body?: BodyInit, json = false): Promise<void> {
  const res = await platformFetch(path, {
    method,
    headers: json ? { "Content-Type": "application/json" } : {},
    body,
    credentials: "same-origin",
  });
  if (!res.ok) {
    const j = (await res.json().catch(() => ({}))) as { error?: string; reason?: string };
    throw new DocError(res.status, refusalOf(res.status, j, `HTTP ${res.status}`));
  }
}

/** Make the Doc's public link, or put its current content to the one it
 * has (Update): seal `v` as the next version, upload it, and have the
 * platform record it. The Doc is read again first, so what's sealed is what
 * is there. */
export async function sharePublic(project: string, id: string): Promise<ProjectDocPublic> {
  const v = await projectDocs.get(project, id);
  const link = await projectDocs.makePublic(project, id);
  const { manifest, blobs } = await snapshotOf(project, v);
  const ver = link.ver + 1;
  const parts = await sealParts(fromBase64Url(link.key), link.id, ver, snapshotBytes(manifest, blobs));
  for (let i = 0; i < parts.length; i++) {
    await control("PUT", `/api/doc-links/${link.id}/v/${ver}/parts/${i}`, parts[i] as BodyInit);
  }
  const size = parts.reduce((n, p) => n + p.length, 0);
  await control("POST", `/api/doc-links/${link.id}/v/${ver}/commit`, JSON.stringify({ parts: parts.length, size }), true);
  return projectDocs.servedPublic(project, id, ver, v.doc.rev);
}

/** Take the link down: control first (a 404 is as good), then the platform's
 * record, so a failure between them leaves a link that can be revoked again. */
export async function revokePublic(project: string, id: string, p: ProjectDocPublic): Promise<void> {
  try {
    await control("DELETE", `/api/doc-links/${p.id}`);
  } catch (e) {
    if (!(e instanceof DocError && e.status === 404)) throw e;
  }
  await projectDocs.forgetPublic(project, id);
}
