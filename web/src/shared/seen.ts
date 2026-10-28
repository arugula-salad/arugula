// What this person has seen (DECISIONS.md "Hints come from facts, once per
// account"): the hints already mentioned (`seen:<id>`), whether they want
// tips at all (`tips`) and big ones (`tips.modal`), the last version What's
// new showed. Per account
// on control's page (an account pref on the hub, `/api/me/prefs`), else
// per browser (localStorage). The page says which with `setSeenBackend`;
// until it does, and until the prefs have loaded once, nothing counts as
// unseen, so a hint never shows before we know.

/** Prefs by name, each a short JSON value; `null` takes one back. */
export type Prefs = Record<string, unknown>;

export interface SeenBackend {
  load(): Promise<Prefs>;
  /** Only the names changed: another tab's other names stay. */
  save(changes: Prefs): Promise<unknown>;
}

const KEY = "arugula.seen";

/** This browser's: one localStorage entry. */
export function localBackend(storage: Pick<Storage, "getItem" | "setItem"> | null = globalThis.localStorage ?? null): SeenBackend {
  const read = (): Prefs => {
    try {
      const v = JSON.parse(storage?.getItem(KEY) ?? "null");
      return v && typeof v === "object" && !Array.isArray(v) ? v : {};
    } catch {
      return {};
    }
  };
  return {
    load: async () => read(),
    save: async (changes) => {
      // Read again first: another tab may have written since.
      const all = { ...read(), ...changes };
      for (const [k, v] of Object.entries(changes)) if (v === null) delete all[k];
      try {
        storage?.setItem(KEY, JSON.stringify(all));
      } catch {
        // Private mode: it lasts for this page.
      }
    },
  };
}

/** The account's, through control: `get` and `set` are its `/api/me/prefs`. */
export function accountBackend(get: () => Promise<Prefs>, set: (changes: Prefs) => Promise<unknown>): SeenBackend {
  return { load: get, save: set };
}

let backend: SeenBackend | null = null;
let prefs: Prefs = {};
let loaded = false;
let loading: Promise<void> | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

/** Where prefs live from now on; they load again from there. */
export function setSeenBackend(b: SeenBackend | null) {
  backend = b;
  prefs = {};
  loaded = false;
  loading = null;
  changed();
  void loadSeen();
}

/** Load once (a second call waits for the first). A backend that can't
 * answer (control too old, or down) gives way to this browser's. */
export function loadSeen(): Promise<void> {
  if (!backend) return Promise.resolve();
  if (loading) return loading;
  const from = backend;
  loading = from.load().then(
    (p) => {
      if (backend !== from) return;
      prefs = { ...p, ...prefs };
      loaded = true;
      changed();
    },
    () => {
      if (backend !== from) return;
      setSeenBackend(localBackend());
    },
  );
  return loading;
}

/** The prefs have loaded: `seen` can be believed. */
export const seenReady = () => loaded;

export function pref(k: string): unknown {
  return prefs[k] ?? null;
}

/** Set one (or take it back with `null`): here at once, saved behind. */
export function setPref(k: string, v: unknown) {
  if (v === null) delete prefs[k];
  else prefs[k] = v;
  changed();
  void backend?.save({ [k]: v }).catch(() => {});
}

/** Hint `id` was mentioned already, or we don't know yet. */
export function seen(id: string): boolean {
  return !loaded || prefs[`seen:${id}`] != null;
}

export function markSeen(id: string, at = Date.now()) {
  setPref(`seen:${id}`, at);
}

/** Tips are on unless this person turned them off. */
export const tipsOn = () => pref("tips") !== false;
export const setTips = (on: boolean) => setPref("tips", on ? null : false);

/** Big tips (pane modals) are on unless this person ticked "Don't show
 * tips like this again" on one, or turned Big tips off in Settings:
 * `tips.modal = "off"`. Chips and toasts don't ask. */
export const bigTipsOn = () => pref("tips.modal") !== "off";
export const setBigTips = (on: boolean) => setPref("tips.modal", on ? null : "off");

export function subscribeSeen(fn: () => void) {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}
