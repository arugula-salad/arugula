// What "needs you" means for a pane, in one place (UI audit #154): the
// Sessions sidebar's groups, Machines' marks, the Swarm and the attention
// drawer read this, so they agree. Nothing counts with it across projects:
// that's the Inbox's, the server's (Lift 6d, #1486). A pane needs you when it asks (a question, a
// permission, input) or when it failed (a command, a PR's checks, a
// program that ended badly). A plain finish is done, not needs-you: the
// Swarm keeps its card a while, counted as finished. Every list of them
// (the Swarm's rail, the phone's sheet, the attention drawer, a session's
// top pane) is in one order, `byNeedsYou` (#451).

import type { PaneInfo, Reason } from "./proto";

/** The one label for it, everywhere. */
export const NEEDS_YOU = "Needs you";

type Info = Pick<PaneInfo, "attention" | "reason" | "type">;

/** Its reason is a failure: a command or checks failed, or it exited. */
export const failedReason = (r: Reason | null | undefined): boolean => r?.kind === "failed" || r?.kind === "exited";

/** It needs you: it asks, or it failed. */
export function needsYou(info: Info): boolean {
  if (info.attention === "needs_input") return true;
  return info.attention === "done" && failedReason(info.reason);
}

/** It finished while nobody was looking, and that's all: done. */
export function finished(info: Info): boolean {
  return info.attention === "done" && !failedReason(info.reason);
}

/** Where it goes in a list of what wants you (#451): what waits on a
 * person first, a gate before the other asks (it expires, and a release
 * waits on it: M34's phone list put it first), then the failures, then
 * what only finished. A failure comes after an ask because nothing is
 * blocked on it now: the run is over, and looking at it can wait for the
 * agent that can't go on until someone answers. Failures also come in
 * batches ("11 failed on build-03"), which mustn't push an ask off a full
 * rail. */
export function needsYouRank(info: Info): number {
  if (info.attention === "needs_input") return info.reason?.kind === "gate" ? 0 : 1;
  return failedReason(info.reason) ? 2 : 3;
}

/** When it started wanting you. */
export const sinceOf = (info: Pick<PaneInfo, "reason" | "ask">): number => info.reason?.since_ms ?? info.ask?.at_ms ?? 0;

/** An item's place in a needs-you list: its rank, when it started
 * wanting you, and a key to break ties. */
export interface Placed {
  rank: number;
  since: number;
  key: string | number;
}

/** The one order of every list of what wants you (#451): the swarm's
 * rail, the phone's sheet, the attention drawer (the bell's and the
 * footer's) and a session's top pane in the sidebar. By rank, then oldest
 * first, so a new one goes to the end of its group and the cards above it
 * don't jump. */
export function byNeedsYou(a: Placed, b: Placed): number {
  return a.rank - b.rank || a.since - b.since || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0);
}

/** A pane's place, keyed by `key`. */
export const placeOf = (info: Info & Pick<PaneInfo, "ask">, key: string | number): Placed => ({ rank: needsYouRank(info), since: sinceOf(info), key });

/** `items` in that order (a copy). */
export function sortNeedsYou<T>(items: readonly T[], place: (t: T) => Placed): T[] {
  return items
    .map((t) => [place(t), t] as const)
    .sort((a, b) => byNeedsYou(a[0], b[0]))
    .map(([, t]) => t);
}

/** Agents: agent blocks, and terminals running one. */
export const isAgent = (p: Pick<PaneInfo, "type" | "kind">) => p.type === "agent" || p.kind === "agent";

/** An agent that runs now: a stopped one (its program ended) doesn't. */
export const agentRunning = (p: Pick<PaneInfo, "type" | "kind" | "running">) => isAgent(p) && p.running;

/** How long a thing has waited, short ("7m", "2h", "3d"); "" when its time
 * is missing or can't be one (0, or before 2001: seconds, not ms), so a row
 * never says it waited since 1970. Its own few lines, not ./time's `ago`:
 * importing that here splits it into a chunk the Doc share page then loads
 * (doc-link.spec). */
export function needAge(since: number | null | undefined, now = Date.now()): string {
  if (!since || !Number.isFinite(since) || since < 1e12) return "";
  const m = Math.max(0, Math.floor((now - since) / 60_000));
  return m < 1 ? "now" : m < 60 ? `${m}m` : m < 1440 ? `${Math.floor(m / 60)}h` : `${Math.floor(m / 1440)}d`;
}
