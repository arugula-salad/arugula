// A machine's Docs, opened in a pane (#1494). The shell's Docs page went
// with Lift 6b (Arugula #1484): Docs are the app's, and a machine's own are
// in Go to… by title. An old `#docs` link still lands: on the app's Docs,
// or Go to… on a machine with no app.

import type { Client } from "../client";
import { opPath, OPS } from "../ops";
import type { DocOpenRequest, DocOpened, DocRow, PaneId } from "../../shared/proto";
import { askText, type PromptChoice } from "../../shared/ui/menu";
import { openGoTo } from "../../shared/kit/frame/goto";
import { landOnPlatform, platformHref } from "../platform-links";

/** The app's Docs, which the shell hands over (`/?to=docs`): the current project's. */
export const DOCS_TO = "/?to=docs";

/** An old `#docs` (the shell's Docs page, retired): the app's Docs, or,
 * with no app, Go to…, where this machine's Docs are. */
export function landOldDocs() {
  if (location.hash !== "#docs") return;
  if (platformHref(DOCS_TO)) return landOnPlatform(DOCS_TO);
  history.replaceState(null, "", location.pathname + location.search);
  window.dispatchEvent(new HashChangeEvent("hashchange"));
  openGoTo();
}

/** Show a Doc from the library in this session (one that is open already
 * comes forward). With `split` a new block
 * opens beside that pane. The block, or the reason it didn't open. */
export async function openDocPane(client: Client, id: string, split?: PaneId): Promise<{ pane: PaneId } | { why: string }> {
  try {
    const body = { id, session: client.session?.toString() ?? null, split } satisfies DocOpenRequest;
    const res = await client.request(OPS["doc.open"].method, opPath("doc.open"), body);
    const v = await res.json<Partial<DocOpened> & { error?: string }>().catch(() => null);
    if (!res.ok || typeof v?.pane !== "number") return { why: v?.error ?? `HTTP ${res.status}` };
    client.showWhenKnown(v.pane);
    return { pane: v.pane };
  } catch (e) {
    return { why: (e as Error).message };
  }
}

/** The same, for a caller that wants only the reason it didn't open, or null. */
export async function openDoc(client: Client, id: string, split?: PaneId): Promise<string | null> {
  const r = await openDocPane(client, id, split);
  return "why" in r ? r.why : null;
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
