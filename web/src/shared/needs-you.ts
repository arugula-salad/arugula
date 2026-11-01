// What "needs you" means, in one place (UI audit #154): the header's bell,
// the footer, the Sessions sidebar's groups and the Swarm's count all read
// this, so they agree. A pane needs you when it asks (a question, a
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

/** It needs you: it asks, or it failed. A remote pane's own machine lists
 * it, so it isn't counted twice. */
export function needsYou(info: Info): boolean {
  if (info.type === "remote") return false;
  if (info.attention === "needs_input") return true;
  return info.attention === "done" && failedReason(info.reason);
}

/** It finished while nobody was looking, and that's all: done. */
export function finished(info: Info): boolean {
  return info.type !== "remote" && info.attention === "done" && !failedReason(info.reason);
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

/** "1 needs you", "3 need you". */
export const needYouText = (n: number) => `${n} ${n === 1 ? "needs" : "need"} you`;
