// Layout (design lift §3.10, Arugula #1497; stored by #1531): a person's
// sentences the agents follow when they arrange a place, each on or off,
// kept on the platform at `/api/layout-prefs` so an agent reads the same.
// Layout only: no role, no behaviour. The PR fold isn't one: what's
// decided has no option. While the platform has no such route (404), there
// is no sheet.

import { useEffect, useState } from "preact/hooks";
import { platformFetch, refusalOf } from "./maintenance";
import { Sheet } from "./kit/sheet";
import { Toggle } from "./kit/field";

/** The sentences, in order, by their key in `control_wire::layout`; each is off until turned on. */
export const LAYOUT_PREFS = [
  { key: "docs_as_tabs", words: "Docs open as tabs" },
  { key: "previews_as_tabs", words: "Previews as tabs" },
] as const;

export type LayoutPrefs = Record<(typeof LAYOUT_PREFS)[number]["key"], boolean>;

const PATH = "/api/layout-prefs";

/** The reader's prefs, a key left out as off; null when the platform has none to keep (404). */
async function read(): Promise<LayoutPrefs | null> {
  const res = await platformFetch(PATH, { credentials: "same-origin" });
  if (res.status === 404) return null;
  const j = (await res.json().catch(() => ({}))) as { prefs?: Partial<LayoutPrefs>; error?: string };
  if (!res.ok) throw new Error(refusalOf(res.status, j, `HTTP ${res.status}`));
  return Object.fromEntries(LAYOUT_PREFS.map((p) => [p.key, j.prefs?.[p.key] === true])) as LayoutPrefs;
}

async function put(prefs: LayoutPrefs): Promise<void> {
  const res = await platformFetch(PATH, { method: "PUT", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ prefs }), credentials: "same-origin" });
  if (!res.ok) throw new Error(refusalOf(res.status, await res.json().catch(() => ({})), `HTTP ${res.status}`));
}

export interface Prefs {
  /** Undefined while they're read; null where the platform keeps none (no sheet). */
  prefs: LayoutPrefs | null | undefined;
  /** Turn one on or off, kept at once; put back, and said by `refused`, if it isn't. */
  flip: (key: keyof LayoutPrefs, on: boolean) => void;
}

export function useLayoutPrefs(refused: (why: string) => void): Prefs {
  const [prefs, setPrefs] = useState<LayoutPrefs | null | undefined>(undefined);
  useEffect(() => {
    let live = true;
    read().then(
      (p) => live && setPrefs(p),
      () => live && setPrefs(null),
    );
    return () => void (live = false);
  }, []);
  return {
    prefs,
    flip: (key, on) => {
      if (!prefs) return;
      const next = { ...prefs, [key]: on };
      setPrefs(next);
      put(next).catch((e: Error) => (setPrefs(prefs), refused(e.message)));
    },
  };
}

/** The Layout sheet (the board's ⋯ → Layout…): each sentence a switch. */
export function LayoutSheet({ prefs, flip, close }: { prefs: LayoutPrefs; flip: Prefs["flip"]; close: () => void }) {
  return (
    <Sheet title="Layout" onClose={close}>
      <div class="layout-prefs" data-layout-prefs>
        {LAYOUT_PREFS.map((p) => (
          <div key={p.key} data-layout-pref={p.key}>
            <Toggle label={p.words} on={prefs[p.key]} onChange={(on) => flip(p.key, on)} />
          </div>
        ))}
      </div>
    </Sheet>
  );
}
