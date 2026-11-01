// The plan board's model (#445): the steps inside each agent session, from
// the task list Claude Code keeps with its todo and task tools (the daemon
// reads it from the transcript: `PaneInfo.plan`), sorted into the board's
// columns. Pure functions, so the scene (plan.ts) and the tests agree;
// plan-of.ts says what each pane brings (`planOf`).
//
// - Pending, In progress, Done: the step's own status.
// - Blocked: a pending step that waits on one that isn't done.
// - Review: what's in progress while its session waits on its person, with
//   why in the words of its needs-you card; a waiting session with no step
//   in progress (or no plan at all) is one Review card of its own.
// - Sessions with no plan and nothing to ask are in a strip below.

import type { PlanStep } from "../../shared/proto";

export type Column = "pending" | "in_progress" | "review" | "blocked" | "done";
export const COLUMNS: Column[] = ["pending", "in_progress", "review", "blocked", "done"];
export const COLUMN_LABEL: Record<Column, string> = {
  pending: "Pending",
  in_progress: "In progress",
  review: "Review",
  blocked: "Blocked",
  done: "Done",
};

/** What the board knows of one agent session (FieldPane.plan). */
export interface PlanOf {
  /** Its task list; empty when it never wrote one. */
  steps: PlanStep[];
  /** The model it answers with, short (`Opus 5.5`), when its transcript says. */
  model: string | null;
  /** The agent, in words ("Claude Code"). */
  agent: string;
  /** The session: what its pane is doing (Claude Code's title). */
  title: string;
  /** It waits on its person: what its needs-you card says. */
  waits: string | null;
}

/** One card on the board. */
export interface Card {
  /** Unique on the board. */
  key: string;
  /** The session's pane. */
  pane: string;
  column: Column;
  /** The step's words: its active wording while in progress. */
  text: string;
  /** Who does it: a teammate the step is given to, else the session's agent. */
  agent: string;
  /** Review: why it waits. */
  why?: string;
  /** Blocked: the steps it waits on, in words. */
  after?: string[];
  /** Done: the session's earlier done steps, which this card stands for
   * too. */
  more?: number;
}

/** The board: each column's cards in session order, and the sessions with
 * neither a plan nor anything to ask. */
export interface Board {
  columns: Record<Column, Card[]>;
  noPlan: string[];
}

/** A session's Done column keeps its last steps; the one before them says
 * how many more there were. */
export const DONE_KEPT = 1;

/** Sort `sessions` (pane key and what the board knows of it, in the order
 * to show them) into the board. */
export function boardOf(sessions: { key: string; plan: PlanOf }[]): Board {
  const columns: Record<Column, Card[]> = { pending: [], in_progress: [], review: [], blocked: [], done: [] };
  const noPlan: string[] = [];
  for (const { key, plan } of sessions) {
    const byId = new Map(plan.steps.map((s) => [s.id, s]));
    const working = plan.steps.filter((s) => s.status === "in_progress");
    if (plan.waits && !working.length) {
      // Nothing in progress to hold: the session itself waits.
      columns.review.push({ key: `${key}#waits`, pane: key, column: "review", text: plan.title, agent: plan.agent, why: plan.waits });
    }
    if (!plan.steps.length) {
      if (!plan.waits) noPlan.push(key);
      continue;
    }
    const done = plan.steps.filter((s) => s.status === "completed");
    const hidden = Math.max(0, done.length - DONE_KEPT);
    for (const s of plan.steps) {
      const card: Card = { key: `${key}#${s.id}`, pane: key, column: columnOf(s, !!plan.waits), text: s.text, agent: s.owner ?? plan.agent };
      if (s.status === "in_progress") card.text = s.active ?? s.text;
      if (card.column === "review") card.why = plan.waits!;
      if (card.column === "blocked") card.after = (s.blocked_by ?? []).map((b) => byId.get(b)?.text ?? `#${b}`);
      if (card.column === "done" && done.indexOf(s) < hidden) continue;
      if (card.column === "done" && hidden && done.indexOf(s) === hidden) card.more = hidden;
      columns[card.column].push(card);
    }
  }
  return { columns, noPlan };
}

/** Where a step goes; `waiting`: its session waits on its person. */
export function columnOf(s: PlanStep, waiting: boolean): Column {
  if (s.status === "completed") return "done";
  if (s.status === "in_progress") return waiting ? "review" : "in_progress";
  return s.blocked_by?.length ? "blocked" : "pending";
}

/** A model's colour, by family: each session's card edge. */
export const MODEL_COL: Record<string, [number, number, number]> = {
  opus: [236, 150, 100],
  sonnet: [110, 185, 255],
  haiku: [150, 225, 140],
};
export const MODEL_OTHER: [number, number, number] = [150, 158, 175];
export const modelCol = (m: string | null): [number, number, number] => MODEL_COL[(m ?? "").split(" ")[0].toLowerCase()] ?? MODEL_OTHER;
