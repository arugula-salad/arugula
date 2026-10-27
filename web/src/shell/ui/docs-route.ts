// The Docs page's route (#1494), in the main bundle so a menu and the
// palette can open it, or a Doc, without loading the page: `#docs`. The
// page itself is a chunk of its own (ui/docs-page.tsx).

import type { Client } from "../client";
import { opPath, OPS } from "../ops";
import type { DocOpenRequest, DocOpened } from "../../shared/proto";

export function docsRoute(): boolean {
  return location.hash === "#docs";
}

export function openDocsPage() {
  if (location.hash !== "#docs") location.hash = "docs";
}

export function closeDocsPage() {
  history.replaceState(null, "", location.pathname + location.search);
  window.dispatchEvent(new HashChangeEvent("hashchange"));
}

/** Show a Doc from the library in this session (one that is open already
 * comes forward), and leave the Docs page for it. The reason it didn't
 * open, or null. */
export async function openDoc(client: Client, id: string): Promise<string | null> {
  try {
    const body = { id, session: client.session?.toString() ?? null } satisfies DocOpenRequest;
    const res = await client.request(OPS["doc.open"].method, opPath("doc.open"), body);
    const v = await res.json<Partial<DocOpened> & { error?: string }>().catch(() => null);
    if (!res.ok || typeof v?.pane !== "number") return v?.error ?? `HTTP ${res.status}`;
    if (docsRoute()) closeDocsPage();
    client.showWhenKnown(v.pane);
    return null;
  } catch (e) {
    return (e as Error).message;
  }
}
