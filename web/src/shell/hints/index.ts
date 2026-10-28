// Hints from facts (DECISIONS.md "Hints come from facts, once per
// account"): when the app sees a fact that makes a feature useful (a port
// listening in a pane), it mentions the feature once, where the fact is.
// Never from a timer or a tour. The entries are in ./list; this is what
// picks one and keeps it on screen.
//
// One on screen at a time, and none new while someone types. A hint is
// seen (shared/seen: per account through control, else per browser) when
// it's shown, or, with `seenOn: "answer"` (every pane modal's), when it's
// answered: acted on or closed. Seen, it never comes back; on screen, it
// stays until it's answered or its fact goes. The Tips switch hides them
// all; Big tips, pane modals alone.

import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { bigTipsOn, markSeen, seen, seenReady, setBigTips, subscribeSeen, tipsOn } from "../../shared/seen";
import { countView, type View } from "../../shared/telemetry";
import { dialogUp, subscribeDialogs } from "../../shared/ui/modal";
import { COMMANDS } from "../ui/commands";
import { factChanged, onFact } from "./facts";
import { HINTS } from "./list";

/** The places a hint can be: in a pane's header, a toast, a view's empty
 * state, or a pane modal over its pane (a big tip). No coach-marks. */
export type Place = "chip" | "toast" | "empty" | "pane-modal";

/** What a hint's fact says now: where (a chip's or a pane modal's pane)
 * and what its copy and action take. */
export interface Fact {
  pane?: PaneId;
  args: Record<string, string | number>;
}

/** Text in a pane modal's body: words, or a key cap (`kbd`). */
export type Inline = string | { kbd: string };
/** A paragraph, or a list's item. */
export type Line = string | Inline[];
/** A pane modal's body is paragraphs and lists. */
export type TipBlock = Line | { list: Line[] };

/** What a button does: a command of `COMMANDS` (ui/commands), given the
 * fact's args, or a route (`#swarm`). Its label can name the fact's
 * values. */
export type Action = { label: string | ((f: Fact) => string) } & ({ command: string } | { route: string });

export interface Hint {
  /** Its name for good: what "seen" remembers (`seen:<id>`). */
  id: string;
  place: Place;
  /** The fact, from what the client knows now; null while it doesn't hold.
   * Asked only while the hint is unseen and Tips are on, so a fact that
   * costs a request stops costing once it's been said. */
  fact(client: Client): Fact | null;
  /** One line: a pane modal's title. */
  copy(f: Fact): string;
  /** Its button (a pane modal's primary one). */
  action?: Action;
  /** A pane modal's: its body, short, and a second button, which only
   * closes it when it has no command or route. */
  body?: (f: Fact) => TipBlock[];
  second?: { label: string } & ({ command: string } | { route: string } | { close: true });
  /** When it's seen: as it shows (a chip's, a toast's), or once it's
   * answered (a pane modal's): one hidden before that (a tab switch, the
   * fact gone) comes back the next time its fact holds. */
  seenOn?: "show" | "answer";
  /** Its own usage counts, beside every pane modal's together: views
   * added to `View` for this hint (control-wire's telemetry.rs). */
  views?: { shown: View; acted: View; closed: View; optedOut: View };
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

const modal = (h: Hint) => h.place === "pane-modal";
const seenOnAnswer = (h: Hint) => (h.seenOn ?? (modal(h) ? "answer" : "show")) === "answer";

/** Someone types: in this page lately, or (the daemon says) someone else
 * in a pane. This person's own keys are the page's: the daemon's `typing`
 * holds for 5 s after them, so a dev server started with Enter would wait
 * that long for its chip. */
function typing(client: Client): boolean {
  if (Date.now() - lastKey < TYPING_MS) return true;
  return !!client.state?.panes.some((p) => p.typing && client.drivenBy(p.id));
}

/** The places drawn now (`chip:<pane>`, `pane-modal:<pane>`, `toast`,
 * `empty`): a hint is only shown where something draws it (a phone has no
 * pane headers, and draws no pane modals). */
const mounted = new Map<string, number>();
const placeOf = (h: Hint, f: Fact) => (h.place === "chip" || h.place === "pane-modal" ? `${h.place}:${f.pane}` : h.place);

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

/** A spec's own hints, before the registry's: only under a test runner
 * (`window.__arugulaTestHints`), so the pane modal can be shown without a
 * real feature's (e2e/hints-pane-modal.spec.ts). */
function testHints(): Hint[] {
  if (!navigator.webdriver) return [];
  return (window as { __arugulaTestHints?: Hint[] }).__arugulaTestHints ?? [];
}

/** Whether `hint` may be drawn with `fact`: somewhere draws it, and a pane
 * modal also needs Big tips on and its pane driven by no one else. */
function may(client: Client, hint: Hint, fact: Fact): boolean {
  if (!mounted.has(placeOf(hint, fact))) return false;
  if (!modal(hint)) return true;
  return bigTipsOn() && fact.pane !== undefined && !client.drivenBy(fact.pane);
}

/** Look again: keep the shown hint while its fact holds, else show the
 * first unseen one whose fact does. */
export function evaluate(client: Client, list: Hint[] = HINTS) {
  const was = shown;
  if (!seenReady() || !tipsOn() || !client.state || quiet()) shown = null;
  else if (shown) {
    const fact = shown.hint.fact(client);
    shown = fact && may(client, shown.hint, fact) ? { hint: shown.hint, fact } : null;
    // The same as it was: nothing to draw again.
    if (was && shown && JSON.stringify(was.fact) === JSON.stringify(shown.fact)) shown = was;
  }
  if (!shown && seenReady() && tipsOn() && client.state && !quiet() && !typing(client)) {
    for (const hint of list === HINTS ? [...testHints(), ...HINTS] : list) {
      if (seen(hint.id)) continue;
      // A big tip never opens under a dialog over the window.
      if (modal(hint) && dialogUp()) continue;
      const fact = hint.fact(client);
      if (!fact || !may(client, hint, fact)) continue;
      shown = { hint, fact };
      if (!seenOnAnswer(hint)) markSeen(hint.id);
      count(hint, "shown");
      break;
    }
  }
  if (shown !== was) changed();
}

/** Counted (opt-in, shared/telemetry): a chip's or a toast's among every
 * hint's, a pane modal's among every pane modal's and, when it names its
 * own, on its own. */
function count(h: Hint, what: "shown" | "acted" | "closed" | "optedOut") {
  if (!modal(h)) {
    countView(what === "shown" ? "hint-shown" : what === "acted" ? "hint-acted" : "hint-dismissed");
    return;
  }
  countView(what === "optedOut" ? "pane-tip-opted-out" : `pane-tip-${what}`);
  if (h.views) countView(h.views[what]);
}

/** Answered (acted on, or closed): seen now if it's seen on answer; with
 * `optOut` ("Don't show tips like this again"), Big tips go off. */
function answer(s: { hint: Hint }, how: "acted" | "closed", optOut: boolean) {
  if (seenOnAnswer(s.hint)) markSeen(s.hint.id);
  count(s.hint, how);
  if (optOut && modal(s.hint)) {
    setBigTips(false);
    count(s.hint, "optedOut");
  }
  shown = null;
  changed();
}

function run(client: Client, s: { fact: Fact }, a: { command: string } | { route: string } | { close: true }) {
  if ("command" in a) COMMANDS[a.command]?.(client, s.fact.args);
  else if ("route" in a) location.hash = a.route;
}

/** Its button (`second`: a pane modal's other one). `optOut`: the pane
 * modal's "Don't show tips like this again" is ticked. */
export function act(client: Client, which: "action" | "second" = "action", optOut = false) {
  const s = shown;
  const a = which === "action" ? s?.hint.action : s?.hint.second;
  if (!s || !a) return;
  answer(s, "close" in a ? "closed" : "acted", optOut);
  run(client, s, a);
}

/** Its × (or a pane modal's Escape). */
export function dismiss(optOut = false) {
  if (shown) answer(shown, "closed", optOut);
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
  // A dialog closing may let a big tip open.
  const offs = [client.subscribe(again), subscribeSeen(again), onFact(again), subscribeDialogs(again)];
  again();
  return () => {
    removeEventListener("keydown", onKey, true);
    clearTimeout(timer);
    offs.forEach((f) => f());
    shown = null;
    changed();
  };
}
