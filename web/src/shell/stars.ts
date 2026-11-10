// Starred tabs and panes (#854): a star is the option `star` on the tab or
// pane, the time it was starred (ms), kept by the daemon with the layout
// and gone when its tab or pane closes. Here without the client, so a test
// can import it.

import type { Intent, Node, Options, PaneId, State, TabId } from "../shared/proto";

/** The option's name. */
export const STAR = "star";

export type Starred = { kind: "tab"; id: TabId; at: number } | { kind: "pane"; id: PaneId; at: number };

/** When a tab or pane was starred, or null. */
export function starOf(options: Options | undefined, kind: "tab" | "pane", id: number): number | null {
  const pairs = (kind === "tab" ? options?.tabs : options?.panes) ?? [];
  const v = pairs.find(([i]) => i === id)?.[1][STAR];
  if (v === undefined) return null;
  const at = Number(v);
  return Number.isFinite(at) ? at : 0;
}

/** Star or unstar a tab or pane. */
export function starIntent(kind: "tab" | "pane", id: number, on: boolean, now = Date.now()): Intent {
  return { op: "set_option", scope: { kind, id }, name: STAR, value: on ? String(now) : null };
}

/** The starred tabs and panes this client sees, oldest star first. Only
 * from its tabs and panes: a guest gets every tab's options. */
export function starredList(state: Pick<State, "tabs" | "panes" | "options"> | null | undefined): Starred[] {
  if (!state) return [];
  const out: Starred[] = [];
  const known = new Set(state.panes.map((p) => p.id));
  for (const t of state.tabs) {
    const at = starOf(state.options, "tab", t.id);
    if (at !== null) out.push({ kind: "tab", id: t.id, at });
    for (const p of panesOf(t.root)) {
      const pat = known.has(p) ? starOf(state.options, "pane", p) : null;
      if (pat !== null) out.push({ kind: "pane", id: p, at: pat });
    }
  }
  return out.sort((a, b) => a.at - b.at || (a.kind === b.kind ? a.id - b.id : a.kind === "tab" ? -1 : 1));
}

function panesOf(n: Node): PaneId[] {
  return n.type === "pane" ? [n.pane] : n.children.flatMap((c) => panesOf(c.node));
}
