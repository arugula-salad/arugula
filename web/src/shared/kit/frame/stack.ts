// The back stack (Lift 3, #1479; v6's): one, across projects. It follows
// the page's router (each place you go, Back and Forward, a slug put right),
// keeps itself in this tab's session storage so a reload keeps it, and holds
// only the real paths the router wrote (aru-082): nothing that wouldn't open
// on its own.
//
// - `esc` goes back a place, as the last thing it does: after an overlay, a
//   menu, the page's own (a zoom, a selection, the Swarm) and a field or a
//   terminal have had it, only when none took it, and only with the keys on
//   nothing or on the frame.
// - Flip back (`⌃Tab`, `g ;`) goes to the last place you were in another
//   project; again goes back.
// - Go to…'s recent places are its newest, one per place.
//
// The page gives it its router (`installStack`); it knows nothing else.

import { addBindings, both, typingIn } from "../../keys";
import { escapeTaken } from "../../ui/menu";
import { dialogUp } from "../../ui/modal";

export interface StackPlace {
  path: string;
  project: string | null;
}

export interface StackRouter {
  /** Where the page is now. */
  here: () => StackPlace;
  go: (path: string) => void;
  onRoute: (fn: () => void) => () => void;
  /** How many times Back or Forward have moved the page. */
  pops: () => number;
}

const KEY = "arugula.stack";
const MAX = 60;

let list: StackPlace[] = [];
let at = -1;

function load() {
  try {
    const v = JSON.parse(sessionStorage.getItem(KEY) ?? "null") as { list?: StackPlace[]; at?: number } | null;
    if (v && Array.isArray(v.list) && typeof v.at === "number") {
      list = v.list.filter((p) => typeof p?.path === "string" && p.path.startsWith("/") && !p.path.startsWith("//"));
      at = Math.min(v.at, list.length - 1);
    }
  } catch {
    // A new stack, then.
  }
}

function save() {
  try {
    sessionStorage.setItem(KEY, JSON.stringify({ list, at }));
  } catch {
    // No storage: the stack lasts as long as the page.
  }
}

/** A path without its slugs (`/p/prj_1-arugula` is `/p/prj_1`): a slug put
 * right is the same place. */
const bare = (path: string) => path.replace(/\b([a-z]+_[0-9a-z]+)-[^/?#]*/g, "$1");

/** The page moved to `p`: a step forward, or Back or Forward (`popped`). */
export function notePlace(p: StackPlace, popped: boolean) {
  const now = list[at];
  if (now && bare(now.path) === bare(p.path)) list[at] = p;
  else if (popped && list[at - 1]?.path === p.path) at--;
  else if (popped && list[at + 1]?.path === p.path) at++;
  else {
    list = [...list.slice(0, at + 1), p].slice(-MAX);
    at = list.length - 1;
  }
  save();
}

/** Places you were, newest first, one per place, not counting this one. */
export function recentPlaces(): StackPlace[] {
  const seen = new Set([bare(list[at]?.path ?? "")]);
  const out: StackPlace[] = [];
  for (let i = list.length - 1; i >= 0; i--) {
    const b = bare(list[i]!.path);
    if (seen.has(b)) continue;
    seen.add(b);
    out.push(list[i]!);
  }
  return out;
}

/** The last place in a project other than this one's. */
export function flipTarget(): StackPlace | null {
  const here = list[at]?.project ?? null;
  for (let i = at - 1; i >= 0; i--) if (list[i]!.project && list[i]!.project !== here) return list[i]!;
  // Forward of here too: after a Back, the place you came back from.
  for (let i = list.length - 1; i > at; i--) if (list[i]!.project && list[i]!.project !== here) return list[i]!;
  return null;
}

export const canBack = () => at > 0;

let router: StackRouter | null = null;

/** Escape's last use: back a place, once nothing else took the key. Not the
 * table's (its listener runs first, before the page's own): this one waits
 * until every listener has had it. */
function lastEscape(e: KeyboardEvent) {
  // A menu that closed on this Escape took it, though it removed itself
  // before this listener ran (a board's ⋯ menu, the account menu).
  if (e.key !== "Escape" || e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || e.isComposing || escapeTaken(e)) return;
  // Only from nowhere or the frame (the rail, the top bar), with nothing
  // open: a page's own control (a menu, a facet, a row) keeps its Escape,
  // even one that doesn't say it took it.
  const at = document.activeElement;
  if (at && at !== document.body && !at.closest("[data-rail], [data-top]")) return;
  if (e.defaultPrevented || typingIn(at) || dialogUp() || document.querySelector('.menu, [role=menu], [aria-haspopup][aria-expanded="true"]')) return;
  const path = location.pathname + location.search;
  setTimeout(() => {
    if (e.defaultPrevented || path !== location.pathname + location.search || !canBack()) return;
    history.back();
  });
}

/** Follow `r`, and take Escape's last use and flip back's keys. Once. */
export function installStack(r: StackRouter): () => void {
  if (router) return () => {};
  router = r;
  load();
  let pops = r.pops();
  notePlace(r.here(), false);
  const off = r.onRoute(() => {
    const popped = r.pops() !== pops;
    pops = r.pops();
    notePlace(r.here(), popped);
  });
  addEventListener("keydown", lastEscape);
  const flip = () => {
    const to = flipTarget();
    if (to) router?.go(to.path);
  };
  const keys = addBindings([
    // Listed for `?`: Escape goes back a place when nothing else takes it (lastEscape).
    { id: "back", label: "Back a place, across projects", group: "switch", keys: both("key:escape"), when: () => false, run: () => {} },
    { id: "flip", label: "Flip to your last place in another project", group: "switch", chip: "flip back", keys: both("Ctrl+Tab", "key:g key:;"), when: () => !!flipTarget(), run: flip },
  ]);
  return () => {
    off();
    keys();
    removeEventListener("keydown", lastEscape);
    router = null;
  };
}
