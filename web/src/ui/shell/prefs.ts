// The new workspace's switches and what this device remembers of it (M108,
// #1023): whether it's on (Appearance → New workspace), the session
// sidebar's width and whether it's open, and whether the window is narrow
// enough that the sidebar folds into the rail.

import { useEffect, useState } from "preact/hooks";
import { appearance, onAppearance } from "../../appearance";
import { usePhone } from "../hooks";

/** Whether this page draws the new workspace: on, and not on a phone. */
export function useNewWorkspace(): boolean {
  const [, tick] = useState(0);
  useEffect(() => onAppearance(() => tick((t) => t + 1)), []);
  const phone = usePhone();
  return appearance().workspace === "new" && !phone;
}

/** A media query's answer, kept current. */
export function useMedia(query: string): boolean {
  const [on, setOn] = useState(() => matchMedia(query).matches);
  useEffect(() => {
    const m = matchMedia(query);
    const fn = () => setOn(m.matches);
    fn();
    m.addEventListener("change", fn);
    return () => m.removeEventListener("change", fn);
  }, [query]);
  return on;
}

/** 700–1100px: the sidebar folds into the rail, and the header and footer
 * drop what's secondary, before the terminals get any smaller. */
export const NARROW = "(max-width: 1100px)";

// ---- the session sidebar, per device

const KEY = "arugula.workspace.sidebar";
export const SIDEBAR_MIN = 200;
export const SIDEBAR_MAX = 380;

interface Sidebar {
  open: boolean;
  width: number;
}

function load(): Sidebar {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "null") as Partial<Sidebar> | null;
    const width = typeof v?.width === "number" ? Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, v.width)) : 248;
    return { open: v?.open !== false, width };
  } catch {
    return { open: true, width: 248 };
  }
}

let sidebar = load();
/** Open over the panes while the window is narrow; never remembered. */
let narrowOpen = false;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(sidebar));
  } catch {
    // Private mode: it lasts for this page.
  }
}

export function useSidebar(): { open: boolean; width: number; narrow: boolean } {
  const [, tick] = useState(0);
  useEffect(() => {
    const fn = () => tick((t) => t + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  const narrow = useMedia(NARROW);
  return { open: narrow ? narrowOpen : sidebar.open, width: sidebar.width, narrow };
}

/** Open or close it; on a narrow window that's the overlay, for now. */
export function setSidebarOpen(open: boolean) {
  if (matchMedia(NARROW).matches) narrowOpen = open;
  else {
    sidebar = { ...sidebar, open };
    save();
  }
  changed();
}

export function sidebarOpen(): boolean {
  return matchMedia(NARROW).matches ? narrowOpen : sidebar.open;
}

export function setSidebarWidth(width: number, keep: boolean) {
  sidebar = { ...sidebar, width: Math.round(Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, width))) };
  if (keep) save();
  changed();
}

/** Somewhere asks the sidebar's search to take the focus (the brand). */
let wantSearch = false;
export function focusSessionSearch() {
  wantSearch = true;
  setSidebarOpen(true);
}
export function takeSearchFocus(): boolean {
  const w = wantSearch;
  wantSearch = false;
  return w;
}
