// What "needs you" means, in one place (UI audit #154): the header's bell,
// the footer, the Sessions sidebar's groups and the Swarm's count all read
// this, so they agree. A pane needs you when it asks (a question, a
// permission, input) or when it failed (a command, a PR's checks, a
// program that ended badly). A plain finish is done, not needs-you: the
// Swarm keeps its card a while, counted as finished.

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

/** Agents: agent blocks, and terminals running one. */
export const isAgent = (p: Pick<PaneInfo, "type" | "kind">) => p.type === "agent" || p.kind === "agent";

/** An agent that runs now: a stopped one (its program ended) doesn't. */
export const agentRunning = (p: Pick<PaneInfo, "type" | "kind" | "running">) => isAgent(p) && p.running;

/** "1 needs you", "3 need you". */
export const needYouText = (n: number) => `${n} ${n === 1 ? "needs" : "need"} you`;
