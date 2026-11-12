// The session menu's Project…: the reader's projects as choices, and the
// project an answer names (Arugula projects RFC). A session's project is
// inferred once from its first linked repo; this sets or clears it by hand.
import type { PlatformProjects, ProjectRef } from "../../shared/proto";
import type { PromptChoice } from "../../shared/ui/menu";

/** The choice that takes the session out of its project. */
export const NO_PROJECT = "none";

/** The reader's projects, the session's own first and marked, then "No
 * project" when it's in one. */
export function projectChoices(all: PlatformProjects, current: ProjectRef | null | undefined): PromptChoice[] {
  const choices: PromptChoice[] = all.projects.map((p) => ({
    value: p.id,
    label: p.name,
    hint: p.team ? "team" : undefined,
    strong: p.id === current?.id,
  }));
  choices.sort((a, b) => Number(!!b.strong) - Number(!!a.strong));
  if (current) choices.push({ value: NO_PROJECT, label: "No project", hint: "take it out of " + current.name });
  return choices;
}

/** What an answer sets: a project (chosen, or typed as its id or its
 * name, any case), `null` for none, or `undefined` when it names none of
 * the reader's projects. */
export function projectPicked(answer: string, all: PlatformProjects): ProjectRef | null | undefined {
  const a = answer.trim();
  if (a === NO_PROJECT || a.toLowerCase() === "no project") return null;
  const p = all.projects.find((x) => x.id === a) ?? all.projects.find((x) => x.name.toLowerCase() === a.toLowerCase());
  return p ? { id: p.id, name: p.name } : undefined;
}
