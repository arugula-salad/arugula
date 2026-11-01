// What an agent's pane brings to the plan board (#445): its task list, its
// model, and what it waits on, in its needs-you card's words. The swarm
// view puts it on each pane it feeds a scene (FieldPane.plan).

import type { FleetPane } from "../fleet";
import { permissionTarget } from "../ui/answer-card";
import { agentDisplay, headlineOf, kindOf, reasonOf, shortPaths, whatItDoes } from "./model";
import type { PlanOf } from "./plan-model";

/** An agent's sessions: agent blocks and terminals running one. Null for
 * anything else, which the board doesn't show. */
export function planOf(p: FleetPane): PlanOf | null {
  if (kindOf(p) !== "agent" || p.info.editor) return null;
  const plan = p.info.plan ?? null;
  const r = reasonOf(p);
  const ask = p.info.ask;
  let waits: string | null = null;
  if (r && p.info.attention === "needs_input" && (r.kind === "ask" || r.kind === "input" || r.kind === "diff")) {
    // As its card says it: "Bash wants to run" its command, or the
    // question asked.
    waits = ask?.kind === "permission" ? `${ask.tool} wants to run ${permissionTarget(ask)}` : headlineOf(r);
  }
  const agent = r?.ask?.agent ?? ask?.agent ?? (p.info.type === "agent" ? "claude" : (p.info.command ?? "claude"));
  return {
    steps: plan?.steps ?? [],
    model: plan?.model ?? null,
    agent: agentName(agent),
    title: whatItDoes(p),
    waits: waits && shortPaths(waits.replace(/\s+/g, " ").trim()),
  };
}

/** "claude-code", "claude", `/usr/bin/claude --resume …`: Claude Code. */
function agentName(id: string): string {
  const program = id.trim().split(/\s+/)[0]?.split(/[\\/]/).pop()?.replace(/\.exe$/i, "") || id;
  return agentDisplay(program);
}
