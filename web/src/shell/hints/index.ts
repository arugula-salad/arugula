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
// stays until it's answered or its fact goes. A hint with `repeat:
// "per-fact"` is seen per fact instead (each fact's `key`): said again for
// each new one, as nvim's key tip is on every nvim start. A menu can show
// one on purpose (`showHint`), seen or not, Tips on or off. The Tips
// switch hides them all; Big tips, pane modals alone.

import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { bigTipsOn, markSeen, markSeenFact, seen, seenFact, seenReady, setBigTips, subscribeSeen, tipsOn } from "../../shared/seen";
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
  /** A repeating hint's: which fact this is (nvim's key tip: the pane and
   * its nvim's pid), what it's seen for. */
  key?: string;
}

/** Text in a pane modal's body: words, a key cap (`kbd`), or a link (it
 * opens in a new tab). */
export type Inline = string | { kbd: string } | { link: string; href: string };
/** A paragraph, or a list's item. */
export type Line = string | Inline[];
/** A pane modal's body is paragraphs, lists and headings. A heading with
 * an `id` is a section a fact can open the modal at (`args.section`). */
export type TipBlock = Line | { list: Line[] } | { heading: string; id?: string };

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
  /** `"per-fact"`: seen per fact, not once: its fact carries a `key`, and
   * a fact with a new key says it again (nvim's key tip, a new nvim
   * process each time). Unset: once, for good. */
  repeat?: "per-fact";
  /** Told each time it comes on screen (again, after a tab switch): nvim's
   * key tip takes its tip for a fact the first time. */
  said?: (f: Fact) => void;
  /** Its own usage counts, beside every hint's (or every pane modal's)
   * together: views added to `View` for this hint (control-wire's
   * telemetry.rs). Opted out is a pane modal's alone. */
  views?: { shown: View; acted: View; closed: View; optedOut?: View };
}

/** The one on screen, and its fact as of now. `asked`: a menu showed it
 * (`showHint`), so neither seen nor Tips keep it off. */
let shown: { hint: Hint; fact: Fact; asked?: boolean } | null = null;
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
/** Said already: for good, or for this fact (a repeating hint's). */
const saidAlready = (h: Hint, f: Fact) => (h.repeat === "per-fact" ? f.key === undefined || seenFact(h.id, f.key) : seen(h.id));
function markSaid(h: Hint, f: Fact) {
  if (h.repeat !== "per-fact") markSeen(h.id);
  else if (f.key !== undefined) markSeenFact(h.id, f.key);
}

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
  if (shown?.asked) {
    // A menu's: while its pane is there and drawn.
    const f = shown.fact;
    if (!client.state || (f.pane !== undefined && !client.info(f.pane)) || !mounted.has(placeOf(shown.hint, f))) shown = null;
  } else if (!seenReady() || !tipsOn() || !client.state || quiet()) shown = null;
  else if (shown) {
    const fact = shown.hint.fact(client);
    shown = fact && may(client, shown.hint, fact) ? { hint: shown.hint, fact } : null;
    // Another fact of a repeating hint: it's said anew, below.
    if (shown?.hint.repeat === "per-fact" && shown.fact.key !== was?.fact.key) shown = null;
    // The same as it was: nothing to draw again.
    if (was && shown && JSON.stringify(was.fact) === JSON.stringify(shown.fact)) shown = was;
  }
  if (!shown && seenReady() && tipsOn() && client.state && !quiet() && !typing(client)) {
    for (const hint of list === HINTS ? [...testHints(), ...HINTS] : list) {
      if (hint.repeat !== "per-fact" && seen(hint.id)) continue;
      // A big tip never opens under a dialog over the window.
      if (modal(hint) && dialogUp()) continue;
      const fact = hint.fact(client);
      if (!fact || saidAlready(hint, fact) || !may(client, hint, fact)) continue;
      shown = { hint, fact };
      if (!seenOnAnswer(hint)) markSaid(hint, fact);
      hint.said?.(fact);
      count(hint, "shown");
      break;
    }
  }
  if (shown !== was) changed();
}

/** Show `hint` with `fact` now, from a menu (nvim's help card from the pane
 * menu or Help): seen or not, Tips on or off, in place of whatever was on
 * screen. Says false when nothing draws it there (a phone draws no pane
 * modals): the caller does something else. */
export function showHint(client: Client, hint: Hint, fact: Fact): boolean {
  if (!client.state || !mounted.has(placeOf(hint, fact))) return false;
  shown = { hint, fact, asked: true };
  count(hint, "shown");
  changed();
  return true;
}

/** Counted (opt-in, shared/telemetry): a chip's or a toast's among every
 * hint's, a pane modal's among every pane modal's and, when it names its
 * own, on its own. */
function count(h: Hint, what: "shown" | "acted" | "closed" | "optedOut") {
  if (!modal(h)) countView(what === "shown" ? "hint-shown" : what === "acted" ? "hint-acted" : "hint-dismissed");
  else countView(what === "optedOut" ? "pane-tip-opted-out" : `pane-tip-${what}`);
  const own = h.views?.[what];
  if (own) countView(own);
}

/** Answered (acted on, or closed): seen now if it's seen on answer; with
 * `optOut` ("Don't show tips like this again"), Big tips go off. */
function answer(s: { hint: Hint; fact: Fact }, how: "acted" | "closed", optOut: boolean) {
  if (seenOnAnswer(s.hint)) markSaid(s.hint, s.fact);
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
