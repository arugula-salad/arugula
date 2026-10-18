// What the pane toolbar, the pane headers and the keys do to a tab's
// layout (M110, #1025), through the daemon's intents: `arrange` and `focus`
// (M109) and `resize_split`. Nothing here moves a pane by itself.

import { paneIds, type Client } from "../../client";
import type { PaneId, Preset, Rect, TabView } from "../../proto";
import { keyHint } from "../keys";
import type { MenuItem } from "../menu";

export const PRESETS: { preset: Preset; label: string }[] = [
  { preset: "columns", label: "Columns" },
  { preset: "grid", label: "Grid" },
  { preset: "main_stack", label: "Main + stack" },
];

/** A pane's number in its tab, in reading order: what Ctrl+Shift+N goes to
 * (not its id). The same with the tab focused on one pane. */
export function ordinal(tab: TabView, pane: PaneId): number {
  return paneIds(tab).indexOf(pane) + 1;
}

/** Why this person can't change the tab's layout, if they can't. */
export function layoutRefusal(client: Client, tab: TabView): string | null {
  const session = client.sessionOfTab(tab.id) ?? null;
  if (client.role(session) === "viewer") return "Viewers can't change the layout";
  return null;
}

/** Why the presets are off, if they are. */
export function arrangeRefusal(client: Client, tab: TabView): string | null {
  return layoutRefusal(client, tab) ?? (paneIds(tab).length < 2 ? "There's only one pane" : null);
}

/** Rebuild the tab as `preset`; Main + stack makes the active pane the
 * large one. */
export function arrange(client: Client, tab: TabView, preset: Preset, refused?: (why: string) => void) {
  const main = client.active(tab.id);
  client.intent(
    { op: "arrange", tab: tab.id, preset, ...(preset === "main_stack" && main !== undefined ? { main } : {}) },
    refused,
  );
}

/** Fill the tab with `pane` (the active one by default), or, when the tab
 * is focused already, give the layout back. */
export function toggleFocus(client: Client, tab: TabView, pane?: PaneId, refused?: (why: string) => void) {
  const target = pane ?? client.active(tab.id);
  if (tab.focus !== null && (pane === undefined || pane === tab.focus)) {
    client.intent({ op: "focus", tab: tab.id, pane: null }, refused);
  } else if (target !== undefined) {
    client.setActive(target);
    client.intent({ op: "focus", tab: tab.id, pane: target }, refused);
  }
}

/** The pane menu's Focus pane, or Restore layout on the focused one (both
 * shells: the classic bar has no toolbar for it). */
export function focusItems(client: Client, tab: TabView | undefined, id: PaneId): MenuItem[] {
  if (!tab || (tab.focus === null && paneIds(tab).length < 2)) return [];
  const restore = tab.focus === id;
  return [
    {
      label: restore ? "Restore layout" : "Focus pane",
      shortcut: keyHint("pane.focus"),
      disabled: !!layoutRefusal(client, tab),
      run: () => toggleFocus(client, tab, id),
    },
  ];
}

/** Show `pane` and give it the keys: the focused one, if its tab is
 * focused on another. */
export function goToPane(client: Client, pane: PaneId) {
  const tab = client.tabOfPane(pane);
  if (tab && tab.focus !== null && tab.focus !== pane) client.intent({ op: "focus", tab: tab.id, pane });
  client.setActive(pane);
  client.viewOf(pane)?.focus();
}

export type Way = "left" | "right" | "up" | "down";

/** Cells a key press moves an edge by. */
const STEP = { cols: 4, rows: 2 };
/** Smallest a neighbour is left with. */
const MIN = 2;

/** The `resize_split` that makes the active pane wider (right), narrower
 * (left), taller (down) or shorter (up): its nearest split that way, its
 * share against the next sibling (the one before, for the last). Null
 * when there's nothing to resize. */
export function resizeFor(
  tab: TabView,
  pane: PaneId,
  way: Way,
  sent?: { split: number; weights: number[] } | null,
): { split: number; weights: number[] } | null {
  const at = tab.layout.panes.find(([id]) => id === pane)?.[1];
  if (!at) return null;
  const across = way === "left" || way === "right";
  const inside = (r: Rect) => at.x >= r.x && at.y >= r.y && at.x + at.cols <= r.x + r.cols && at.y + at.rows <= r.y + r.rows;
  const nearest = tab.layout.splits
    .filter((s) => (across ? s.dir === "row" : s.dir === "column") && inside(s.rect))
    .sort((a, b) => a.rect.cols * a.rect.rows - b.rect.cols * b.rect.rows)[0];
  if (!nearest) return null;
  // Which child holds the pane: children are laid end to end, a one-cell
  // gutter between each.
  let start = across ? nearest.rect.x : nearest.rect.y;
  const pos = across ? at.x : at.y;
  let k = -1;
  nearest.extents.forEach((e, i) => {
    if (pos >= start && pos < start + e) k = i;
    start += e + 1;
  });
  if (k < 0) return null;
  const n = k + 1 < nearest.extents.length ? k + 1 : k - 1;
  const step = across ? STEP.cols : STEP.rows;
  const grow = way === "right" || way === "down";
  // From what was sent last, when the daemon hasn't answered it yet (a key
  // held down, or pressed twice quickly).
  const ext = (sent?.split === nearest.id && sent.weights.length === nearest.extents.length ? sent.weights : nearest.extents).slice();
  const d = grow ? Math.min(step, ext[n] - MIN) : -Math.min(step, ext[k] - MIN);
  if (d === 0 || (grow && d < 0) || (!grow && d > 0)) return null;
  ext[k] += d;
  ext[n] -= d;
  return { split: nearest.id, weights: ext };
}

/** The last resize sent, and the state it was sent from. */
let sent: { split: number; weights: number[]; rev: number | undefined } | null = null;

/** Grow or shrink the shown tab's active pane; false if nothing could. */
export function resizeActive(client: Client, way: Way): boolean {
  const tab = client.tabView();
  const pane = tab ? client.active(tab.id) : undefined;
  if (!tab || pane === undefined || tab.focus !== null) return false;
  if (layoutRefusal(client, tab)) return false;
  const rev = client.state?.rev;
  const r = resizeFor(tab, pane, way, sent?.rev === rev ? sent : null);
  if (!r) return false;
  sent = { ...r, rev };
  client.intent({ op: "resize_split", split: r.split, weights: r.weights });
  return true;
}
