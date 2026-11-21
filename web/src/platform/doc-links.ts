// The Docs the reader shared by link (Arugula #1315): the hub's `GET
// /api/doc-links`, on the app's host. A host that answers 404 shows no
// Shared by you. The
// hub knows a link's id, the machine that shared it, its size, dates and a
// rough count of opens, and nothing of what it says (the key is on the
// machine), so a link's title, its URL and opening it are the machine's
// (`PlatformApi.docLinks`).

import { platformFetch, refusalOf } from "../shared/maintenance";

/** One row of the hub's `GET /api/doc-links`. */
export interface DocLink {
  id: string;
  daemon: string;
  size: number;
  made: number;
  updated: number;
  opens: number;
}

/** `null` while it's asked; `absent` where this host doesn't serve it. */
export type DocLinksState = null | "absent" | { error: string } | DocLink[];

let state: DocLinksState = null;
let asking: Promise<void> | null = null;
const listeners = new Set<() => void>();
const changed = (s: DocLinksState) => {
  state = s;
  for (const fn of [...listeners]) fn();
};

async function read() {
  try {
    const r = await platformFetch("/api/doc-links", { credentials: "same-origin" });
    if (r.status === 404) return changed("absent");
    const v = (await r.json().catch(() => ({}))) as DocLink[] | { links?: DocLink[]; error?: string; reason?: string };
    if (!r.ok) return changed({ error: refusalOf(r.status, Array.isArray(v) ? {} : v) });
    changed(Array.isArray(v) ? v : (v.links ?? []));
  } catch (e) {
    changed({ error: (e as Error).message });
  }
}

export const docLinks = {
  get: () => state,
  /** Whether this host serves them: known once the first read answers. */
  served: () => Array.isArray(state),
  subscribe(fn: () => void) {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  },
  /** Read them, once per page unless `again`. */
  load(again = false): Promise<void> {
    if (!asking || again) asking = read();
    return asking;
  },
  /** Take a link down at the hub (`DELETE /api/doc-links/{id}`), then read them again. */
  async remove(id: string) {
    const r = await platformFetch(`/api/doc-links/${encodeURIComponent(id)}`, { method: "DELETE", credentials: "same-origin" });
    if (!r.ok) throw new Error(refusalOf(r.status, (await r.json().catch(() => ({}))) as { error?: string }));
    await docLinks.load(true);
  },
};
