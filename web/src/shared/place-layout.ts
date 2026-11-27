// A place's layout (design lift §3.12, Arugula #1497; the record is
// #1496's, control-wire's `layout`): the tabs and panes the person and their
// agents arranged on a board (and, in Lift 9, the Overview), read from
// `/api/layouts/{place}` and kept live by the stream's `layout` event.
//
// - The record has no current tab: which tab shows is this page's, so an
//   agent's tab never becomes current and its pane never takes the keys.
// - Busy: if the person typed, scrolled, clicked or switched a tab in the
//   last 4 seconds, a pane an agent opens arrives without motion and with a
//   dot (`quiet`), until they look at it.

import { useEffect, useRef, useState } from "preact/hooks";
import type { LayoutChange, LayoutChanged, LayoutPane, PlaceLayout } from "./proto";
import { chat } from "./chat-store";
import { platformFetch, refusalOf } from "./maintenance";

/** The place's own tab, always first (`HOME_TAB`). */
export const HOME = "home";
/** How recently the person did something for an agent's pane to arrive quietly, in ms. */
export const BUSY_MS = 4_000;

/** A place's record before anyone changed it: its default, `rev` 0. */
export const blank = (place: string): PlaceLayout => ({ place, rev: 0, tabs: [{ id: HOME, title: "", opened_by: { account: "", machine: null }, opened_at: 0, for_you: false, panes: [] }], closed_defaults: [], default_sizes: {}, closed: [], changed: null });

async function call<T>(method: string, place: string, body?: unknown): Promise<T> {
  const res = await platformFetch(`/api/layouts/${encodeURIComponent(place)}`, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  const j = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) throw new LayoutError(res.status, refusalOf(res.status, j, `HTTP ${res.status}`));
  return j;
}

class LayoutError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/** A change made in the page alone, while the platform has no layouts (a
 * hub before #1496's server release): the person's own changes hold until
 * the page goes, and none is kept. */
export function applyHere(was: PlaceLayout, c: LayoutChange, now = Date.now()): LayoutChanged {
  const l: PlaceLayout = structuredClone(was);
  const you = { account: "", machine: null };
  const id = (p: string) => `${p}_here${l.rev + 1}`;
  const find = (pane: string) => l.tabs.flatMap((t) => t.panes.map((p) => [t, p] as const)).find(([, p]) => p.id === pane);
  const home = () => (l.changed = { by: you, at: now });
  const out: LayoutChanged = { layout: l, pane: null, tab: null, closed: [] };
  switch (c.op) {
    case "show": {
      const p: LayoutPane = { id: id("pan"), kind: c.pane.kind, ref: c.pane.ref, title: c.pane.title, machine: c.pane.machine, state: c.pane.state, opened_by: you, opened_at: now, touched: false, in_place_of: c.in_place_of, size: null, offline: false };
      const t = l.tabs.find((x) => x.id === c.tab) ?? l.tabs[0]!;
      t.panes.push(p);
      if (t.id === HOME) home();
      Object.assign(out, { pane: p.id, tab: t.id });
      break;
    }
    case "move": {
      const w = find(c.pane);
      if (!w) break;
      w[0].panes = w[0].panes.filter((p) => p !== w[1]);
      w[1].touched = true;
      let t = l.tabs.find((x) => x.id === c.tab);
      if (!t) l.tabs.push((t = { id: id("tab"), title: w[1].title, opened_by: you, opened_at: now, for_you: false, panes: [] }));
      t.panes.splice(c.index ?? t.panes.length, 0, w[1]);
      if (w[0].id === HOME) home();
      Object.assign(out, { pane: w[1].id, tab: t.id });
      break;
    }
    case "resize":
    case "maximise": {
      const w = find(c.pane);
      if (w) (w[1].touched = true), c.op === "resize" && (w[1].size = c.size);
      else if (c.op === "resize") l.default_sizes[c.pane] = c.size;
      home();
      break;
    }
    case "close": {
      const w = find(c.pane);
      if (w) (w[0].panes = w[0].panes.filter((p) => p !== w[1])), l.closed.unshift(w[1]);
      else l.closed_defaults.push(c.pane);
      if (!w || w[0].id === HOME) home();
      break;
    }
    case "close_tab": {
      const t = l.tabs.find((x) => x.id === c.tab && x.id !== HOME);
      if (t) (l.tabs = l.tabs.filter((x) => x !== t)), l.closed.unshift(...t.panes);
      break;
    }
    case "reopen": {
      const p = l.closed.find((x) => x.id === c.pane);
      if (p) (l.closed = l.closed.filter((x) => x !== p)), l.tabs[0]!.panes.push(p), home();
      break;
    }
    case "reset":
      l.closed.unshift(...l.tabs[0]!.panes);
      l.tabs[0]!.panes = [];
      l.closed_defaults = [];
      l.default_sizes = {};
      l.changed = null;
      break;
  }
  l.closed = l.closed.slice(0, 20);
  l.rev++;
  return out;
}

export const placeLayouts = {
  read: (place: string) => call<PlaceLayout>("GET", place),
  change: (place: string, c: LayoutChange) => call<LayoutChanged>("POST", place, c),
};

/** When the person last did something on the page (a key, the wheel, a click, a tab). */
let lastBusy = 0;
export const busyNow = () => (lastBusy = Date.now());
export const busy = (now = Date.now()) => now - lastBusy < BUSY_MS;
if (typeof window !== "undefined") for (const k of ["keydown", "wheel", "pointerdown"]) window.addEventListener(k, busyNow, { capture: true, passive: true });

/** Each of the layout's panes, with its tab. */
export const panesOf = (l: PlaceLayout): { pane: LayoutPane; tab: string }[] => l.tabs.flatMap((t) => t.panes.map((pane) => ({ pane, tab: t.id })));

export interface Arranged {
  /** The record; the place's default (rev 0) until it's read, or when it can't be. */
  layout: PlaceLayout;
  /** Read from the platform at least once. */
  read: boolean;
  /** Panes an agent opened since the page read the record while the person was busy: their dot, until looked at. */
  quiet: ReadonlySet<string>;
  /** Panes an agent opened since the page first read it (they slide in). */
  arrived: ReadonlySet<string>;
  /** Look at a pane: its dot goes. */
  seen: (pane: string) => void;
  /** Send a change; the record it answers replaces this one. Null on a refusal, said by `refused`. */
  change: (c: LayoutChange) => Promise<LayoutChanged | null>;
}

/** `place`'s layout, read again on its `layout` event (unless the page has that rev) and when the stream comes back. */
export function usePlaceLayout(place: string | null, refused: (why: string) => void): Arranged {
  const [layout, setLayout] = useState<PlaceLayout | null>(null);
  const [quiet, setQuiet] = useState<ReadonlySet<string>>(new Set());
  const [arrived, setArrived] = useState<ReadonlySet<string>>(new Set());
  const known = useRef<Set<string> | null>(null);
  const rev = useRef(-1);
  /** The platform has no layouts yet (404): changes are this page's alone. */
  const here = useRef(false);
  const now = useRef<PlaceLayout | null>(null);
  now.current = layout;
  const take = (l: PlaceLayout, mine = false) => {
    if (l.rev < rev.current) return;
    rev.current = l.rev;
    const ids = panesOf(l).map((p) => p.pane);
    // New panes an agent opened (not this page's own change): quiet if the person is busy.
    if (known.current && !mine) {
      const fresh = ids.filter((p) => !known.current!.has(p.id) && p.opened_by.machine);
      if (fresh.length) {
        const ids = fresh.map((p) => p.id);
        setArrived((s) => new Set([...s, ...ids]));
        if (busy()) setQuiet((s) => new Set([...s, ...ids]));
      }
    }
    known.current = new Set(ids.map((p) => p.id));
    now.current = l;
    setLayout(l);
  };
  useEffect(() => {
    if (!place) return;
    let live = true;
    rev.current = -1;
    known.current = null;
    here.current = false;
    setLayout(null);
    const read = () =>
      here.current
        ? Promise.resolve()
        : placeLayouts.read(place).then(
            (l) => live && take(l),
            (e: LayoutError) => {
              if (!live) return;
              if (e.status === 404) here.current = true;
              if (rev.current < 0) take(blank(place));
            },
          );
    void read();
    const off = chat.onObject(place, () => void read());
    return () => {
      live = false;
      off();
    };
  }, [place]);
  return {
    layout: layout ?? blank(place ?? ""),
    read: layout !== null,
    quiet,
    arrived,
    seen: (pane) => quiet.has(pane) && setQuiet((s) => new Set([...s].filter((p) => p !== pane))),
    change: async (c) => {
      if (!place) return null;
      if (here.current) {
        const r = applyHere(now.current ?? blank(place), c);
        take(r.layout, true);
        return r;
      }
      try {
        const r = await placeLayouts.change(place, c);
        take(r.layout, true);
        return r;
      } catch (e) {
        refused((e as Error).message);
        return null;
      }
    },
  };
}
