// What dropping a dragged pane or tab does: onto a pane's edge, into the
// tab bar.

import { useEffect } from "preact/hooks";
import type { Client } from "../../client";
import type { Intent, TabId } from "../../proto";
import { drag, type Dragged, type Target } from "../drag";
import { closeMenu } from "../menu";

export function dropAllowed(client: Client, what: Dragged, t: Exclude<Target, null>): boolean {
  if (t.kind !== "pane") return true;
  if (what.kind === "pane") return what.pane !== t.pane;
  return t.edge !== "center" && client.tabOfPane(t.pane)?.id !== what.tab;
}

export function drop(client: Client, what: Dragged, target: Target) {
  closeMenu();
  if (!target || !dropAllowed(client, what, target)) return;
  const session = client.state?.sessions.find((s) => s.id === client.session);
  if (!session) return;
  let intent: Intent | null = null;
  if (what.kind === "pane" && target.kind === "pane") {
    intent = { op: "move_pane", pane: what.pane, target: target.pane, edge: target.edge };
  } else if (what.kind === "pane" && target.kind === "tabbar") {
    intent = { op: "break_pane", pane: what.pane, session: session.id, index: Math.min(target.index, session.tabs.length) };
  } else if (what.kind === "tab" && target.kind === "pane") {
    intent = { op: "dock_tab", tab: what.tab, target: target.pane, edge: target.edge };
  } else if (what.kind === "tab" && target.kind === "tabbar") {
    const from = session.tabs.indexOf(what.tab);
    let to = Math.min(target.index, session.tabs.length);
    if (from !== -1 && to > from) to -= 1;
    if (to !== from) intent = { op: "move_tab", tab: what.tab, session: session.id, index: to };
  }
  if (intent) client.intent(intent);
}

/** While dragging a pane, resting on a tab for a moment switches to it,
 * so a pane can be dropped into another tab's layout. */
export function useHoverToSwitchTabs(client: Client) {
  useEffect(() => {
    let timer: number | undefined;
    let over: TabId | null = null;
    return drag.subscribe(() => {
      const d = drag.current;
      const el = d && document.elementFromPoint(d.x, d.y)?.closest<HTMLElement>("[data-tab-id]");
      const id = el ? Number(el.dataset.tabId) : null;
      if (id === over) return;
      over = id;
      clearTimeout(timer);
      if (d?.what.kind === "pane" && id !== null && id !== client.tab) {
        timer = window.setTimeout(() => client.selectTab(id), 500);
      }
    });
  }, [client]);
}
