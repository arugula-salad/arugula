// The account's Views: one pref each (`view:<id>`), through the same store as
// the hints it has seen, so they follow the account on control's page and stay
// in this browser without it. A key per View means two devices changing
// different Views don't collide; the same View is last write wins.

import { prefsStarting, pref, reloadPrefs, setPref, subscribeSeen } from "../../shared/seen";
import { decode, encode, fits, insert, leaves, bestSpot, remove, type Address, type Edge, type ViewDoc, type ViewNode, MAX_LEAVES, MAX_NAME } from "./model";

const PREFIX = "view:";
const LAST_KEY = "arugula.views.last";

export interface SavedView {
  id: string;
  doc: ViewDoc;
}

/** Every View, oldest first: the order they sit beside the tabs in. */
export function listViews(): SavedView[] {
  return prefsStarting(PREFIX)
    .flatMap(([k, v]) => {
      const doc = decode(v);
      return doc ? [{ id: k.slice(PREFIX.length), doc }] : [];
    })
    .sort((a, b) => a.doc.at - b.doc.at || a.id.localeCompare(b.id));
}

export function getView(id: string): ViewDoc | null {
  return decode(pref(PREFIX + id));
}

export const subscribeViews = subscribeSeen;

export function newViewId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(8));
  return Array.from(bytes, (b) => (b % 36).toString(36)).join("");
}

/** A View with nothing left in it isn't kept. */
function put(id: string, doc: ViewDoc | null) {
  setPref(PREFIX + id, doc && encode(doc));
}

/** Set a View's tree. Null: it's gone. False: it would no longer fit. */
export function saveTree(id: string, t: ViewNode | null): boolean {
  if (t === null) {
    put(id, null);
    return true;
  }
  const doc: ViewDoc = { ...(getView(id) ?? { at: Date.now() }), t };
  if (!fits(doc)) return false;
  put(id, doc);
  return true;
}

export function renameView(id: string, name: string) {
  const doc = getView(id);
  if (!doc) return;
  const n = name.trim().slice(0, MAX_NAME);
  const { n: _old, ...rest } = doc;
  put(id, n ? { ...rest, n } : rest);
}

export function deleteView(id: string) {
  put(id, null);
}

/** Why `a` can't go in this View, if it can't. */
export function addRefusal(t: ViewNode | null, a: Address): string | null {
  const now = leaves(t);
  if (now.includes(a)) return null;
  return now.length >= MAX_LEAVES ? `A view holds ${MAX_LEAVES} blocks.` : null;
}

/** Put a pane in a View, where there's most room, or at a spot; the tree
 * after, or null when it can't go in. Moving one already there is fine. */
export function placeIn(id: string, a: Address, box: { x: number; y: number; w: number; h: number }, spot?: { at: Address | null; edge: Edge }): ViewNode | null {
  const before = remove(getView(id)?.t ?? null, a);
  if (addRefusal(before, a)) return null;
  const where = spot ?? bestSpot(before, box);
  const t = insert(before, where?.at === a ? null : (where?.at ?? null), where?.edge ?? "center", a);
  return saveTree(id, t) ? t : null;
}

export function lastView(): string | null {
  try {
    const id = localStorage.getItem(LAST_KEY);
    return id && getView(id) ? id : null;
  } catch {
    return null;
  }
}

export function noteLastView(id: string) {
  try {
    localStorage.setItem(LAST_KEY, id);
  } catch {
    // not remembered
  }
}

// Another device may have changed a View: look again when the page comes back.
if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "visible") void reloadPrefs();
  });
}
