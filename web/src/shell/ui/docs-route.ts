// The Docs page's route (#1494), in the main bundle so a menu and the
// palette can open it, or a Doc, without loading the page: `#docs`. The
// page itself is a chunk of its own (ui/docs-page.tsx).

import type { Client } from "../client";
import { opPath, OPS } from "../ops";
import type { DocOpenRequest, DocOpened, DocRow, PaneId } from "../../shared/proto";
import { askText, type PromptChoice } from "../../shared/ui/menu";

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
 * comes forward), and leave the Docs page for it. With `split` a new block
 * opens beside that pane. The reason it didn't open, or null. */
export async function openDoc(client: Client, id: string, split?: PaneId): Promise<string | null> {
  try {
    const body = { id, session: client.session?.toString() ?? null, split } satisfies DocOpenRequest;
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

/** "Doc…": one of this machine's Docs, beside `from`, or brought forward
 * where it's open already. */
export async function pinDoc(client: Client, o: { from: PaneId }) {
  const choices = (async (): Promise<PromptChoice[]> => {
    const res = await client.request(OPS["docs.list"].method, opPath("docs.list"));
    const rows = await res.json<DocRow[]>().catch(() => null);
    if (!res.ok || !Array.isArray(rows)) return [];
    return [...rows]
      .sort((a, b) => b.updated - a.updated)
      .map((r) => ({ value: r.id, label: r.open != null ? `${r.title} · open` : r.title }));
  })().catch(() => []);
  const v = (await askText("Doc", "", "Doc title", choices))?.trim();
  if (!v) return;
  const typed = v.toLowerCase();
  const all = await choices;
  const hit = all.find((c) => c.label.toLowerCase() === typed) ?? all.find((c) => c.label.toLowerCase().replace(/ · open$/, "") === typed);
  if (!hit) return client.toast(`No Doc here is called ${v}`);
  const why = await openDoc(client, hit.value, o.from);
  if (why) client.toast(why);
}
