// The phone offer's fact (Arugula issue iss_a2ad28a0f67e61e8fe081d8121e37c3e,
// the onboarding prototype's tab 4b): a pane sat at a prompt for 5 minutes
// or more while the person was away (the window hidden or not focused, or
// no key or pointer for 5 minutes), on a machine with no account. It's said
// when they come back, once a day at most (its key is the day), and never
// on a joined machine: there, the phone has it already.

import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import type { Fact } from "./index";
import { factChanged } from "./facts";
import { controlState, subscribeControlState } from "../ui/control-state";

export const AWAY_MS = 5 * 60 * 1000;

/** When they stepped away (hidden, blurred), null while they're here. */
let leftAt: number | null = null;
let lastInput = Date.now();
/** When this page first saw each pane waiting, for one with no reason. */
const waitingSince = new Map<PaneId, number>();
/** What they came back to: the pane that waited longest while they were
 * away, how long it had waited, and the day, the fact's key: said once a
 * day, and kept on screen once said, though the prompt is answered. */
let cameBack: { pane: PaneId; minutes: number; day: string } | null = null;
let client: Client | null = null;
let watching = false;

/** Today, as the fact's key (2026-11-23). */
const today = () => new Date().toLocaleDateString("en-CA");

const here = () => document.visibilityState === "visible" && document.hasFocus();

function waiting(c: Client): { pane: PaneId; since: number }[] {
  const now = Date.now();
  const panes = (c.state?.panes ?? []).filter((p) => p.attention === "needs_input");
  for (const id of waitingSince.keys()) if (!panes.some((p) => p.id === id)) waitingSince.delete(id);
  return panes.map((p) => {
    if (!waitingSince.has(p.id)) waitingSince.set(p.id, now);
    return { pane: p.id, since: p.reason?.since_ms ?? waitingSince.get(p.id)! };
  });
}

/** Input, or the window back: if they were away, what waited for them. */
function back() {
  const now = Date.now();
  const from = leftAt ?? (now - lastInput >= AWAY_MS ? lastInput : null);
  lastInput = now;
  if (!here()) {
    leftAt ??= now;
    return;
  }
  leftAt = null;
  if (from === null || !client) return;
  let best: { pane: PaneId; since: number } | null = null;
  for (const w of waiting(client)) if (now - Math.max(w.since, from) >= AWAY_MS && (!best || w.since < best.since)) best = w;
  if (best) {
    cameBack = { pane: best.pane, minutes: Math.floor((now - best.since) / 60_000), day: today() };
    factChanged();
  }
}

/** Hidden, or blurred: once focus settles, since a frame in a pane (a
 * block's site) takes it with a blur too. */
function away() {
  const at = Date.now();
  setTimeout(() => {
    if (!here()) leftAt ??= at;
  });
}

function watch() {
  watching = true;
  for (const e of ["keydown", "pointerdown", "pointermove", "wheel"]) addEventListener(e, back, { capture: true, passive: true });
  addEventListener("focus", back);
  addEventListener("blur", away);
  document.addEventListener("visibilitychange", () => (document.visibilityState === "visible" ? back() : away()));
  subscribeControlState(factChanged);
}

export function waitedWhileAway(c: Client): Fact | null {
  client = c;
  if (!watching) watch();
  // Only the owner's own page knows (control-state), and only says
  // not_joined with no account.
  if (controlState()?.state !== "not_joined") return null;
  waiting(c);
  return cameBack && { key: cameBack.day, args: { pane: cameBack.pane, minutes: cameBack.minutes } };
}
