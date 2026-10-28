// Hints from facts (DECISIONS.md "Hints come from facts, once per
// account"): when the app sees a fact that makes a feature useful (a port
// listening in a pane), it mentions the feature once, where the fact is.
// Never from a timer or a tour. The entries are in ./list; this is what
// picks one and keeps it on screen.
//
// One on screen at a time, and none new while someone types. Once shown,
// a hint is seen (shared/seen: per account through control, else per
// browser), so it never comes back; it stays until it's acted on,
// dismissed (×), or its fact goes. The Tips switch hides them all.

import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { markSeen, seen, seenReady, subscribeSeen, tipsOn } from "../../shared/seen";
import { countView } from "../../shared/telemetry";
import { COMMANDS } from "../ui/commands";
import { factChanged, onFact } from "./facts";
import { HINTS } from "./list";

/** The three places a hint can be: in a pane's header, a toast, or a view's
 * empty state. No modals and no coach-marks. */
export type Place = "chip" | "toast" | "empty";

/** What a hint's fact says now: where (a chip's pane) and what its copy and
 * action take. */
export interface Fact {
  pane?: PaneId;
  args: Record<string, string | number>;
}

export interface Hint {
  /** Its name for good: what "seen" remembers (`seen:<id>`). */
  id: string;
  place: Place;
  /** The fact, from what the client knows now; null while it doesn't hold.
   * Asked only while the hint is unseen and Tips are on, so a fact that
   * costs a request stops costing once it's been said. */
  fact(client: Client): Fact | null;
  /** One line. */
  copy(f: Fact): string;
  /** What its button does: a command of `COMMANDS` (ui/commands), given
   * the fact's args, or a route (`#swarm`). Its label can name the fact's
   * values. */
  action?: { label: string | ((f: Fact) => string) } & ({ command: string } | { route: string });
}

/** The one on screen, and its fact as of now. */
let shown: { hint: Hint; fact: Fact } | null = null;
let lastKey = 0;
/** Typing in the page holds new hints back this long after the last key. */
export const TYPING_MS = 1500;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export const shownHint = () => shown;

export function subscribeHints(fn: () => void) {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

/** Someone types: in this page lately, or (the daemon says) someone else
 * in a pane. This person's own keys are the page's: the daemon's `typing`
 * holds for 5 s after them, so a dev server started with Enter would wait
 * that long for its chip. */
function typing(client: Client): boolean {
  if (Date.now() - lastKey < TYPING_MS) return true;
  return !!client.state?.panes.some((p) => p.typing && client.drivenBy(p.id));
}

/** The places drawn now (`chip:<pane>`, `toast`, `empty`): a hint is
 * only shown, and so seen, where something draws it (a phone has no pane
 * headers). */
const mounted = new Map<string, number>();
const placeOf = (h: Hint, f: Fact) => (h.place === "chip" ? `chip:${f.pane}` : h.place);

/** A renderer is drawn at `key` until the returned function is called. */
export function mountPlace(key: string): () => void {
  mounted.set(key, (mounted.get(key) ?? 0) + 1);
  factChanged();
  return () => {
    const n = (mounted.get(key) ?? 1) - 1;
    if (n) mounted.set(key, n);
    else mounted.delete(key);
    factChanged();
  };
}

/** Under a test runner (Playwright sets `navigator.webdriver`) hints keep
 * quiet, so a toast never lands on what another spec clicks; the hints'
 * own specs ask for them (`window.__arugulaHints`). */
function quiet(): boolean {
  return !!navigator.webdriver && !(window as { __arugulaHints?: boolean }).__arugulaHints;
}

/** Look again: keep the shown hint while its fact holds, else show the
 * first unseen one whose fact does. */
export function evaluate(client: Client, list: Hint[] = HINTS) {
  const was = shown;
  if (!seenReady() || !tipsOn() || !client.state || quiet()) shown = null;
  else if (shown) {
    const fact = shown.hint.fact(client);
    shown = fact && mounted.has(placeOf(shown.hint, fact)) ? { hint: shown.hint, fact } : null;
    // The same as it was: nothing to draw again.
    if (was && shown && JSON.stringify(was.fact) === JSON.stringify(shown.fact)) shown = was;
  }
  if (!shown && seenReady() && tipsOn() && client.state && !quiet() && !typing(client)) {
    for (const hint of list) {
      if (seen(hint.id)) continue;
      const fact = hint.fact(client);
      if (!fact || !mounted.has(placeOf(hint, fact))) continue;
      shown = { hint, fact };
      markSeen(hint.id);
      countView("hint-shown");
      break;
    }
  }
  if (shown !== was) changed();
}

function close() {
  shown = null;
  changed();
}

/** Its button. */
export function act(client: Client) {
  const s = shown;
  if (!s?.hint.action) return;
  countView("hint-acted");
  close();
  const a = s.hint.action;
  if ("command" in a) COMMANDS[a.command]?.(client, s.fact.args);
  else location.hash = a.route;
}

/** Its ×. */
export function dismiss() {
  if (!shown) return;
  countView("hint-dismissed");
  close();
}

/** Watch `client` for facts until the returned function is called (the
 * page swaps machines, or goes). */
export function watchHints(client: Client): () => void {
  let timer = 0;
  const again = () => evaluate(client);
  const onKey = (e: KeyboardEvent) => {
    if (e.isTrusted === false) return;
    lastKey = Date.now();
    clearTimeout(timer);
    timer = window.setTimeout(again, TYPING_MS + 50);
  };
  addEventListener("keydown", onKey, true);
  const offs = [client.subscribe(again), subscribeSeen(again), onFact(again)];
  again();
  return () => {
    removeEventListener("keydown", onKey, true);
    clearTimeout(timer);
    offs.forEach((f) => f());
    close();
  };
}
