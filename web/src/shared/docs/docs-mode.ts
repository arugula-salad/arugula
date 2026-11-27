// Whose Docs a project's are (Arugula #864): a person's own, or a team's,
// where the list has a Mine filter (Lift 6b). Mine is what the reader made,
// their agents' included: a machine's agent makes a Doc as its owner
// (`author`), with the machine as `agent`. Tested in
// scripts/docs-mode.test.mjs.

import type { PlatformProject, ProjectDoc } from "../proto.gen.ts";

export type DocsMode = { kind: "personal" } | { kind: "team"; project: string; name: string };

export const PERSONAL: DocsMode = { kind: "personal" };

/** A project's mode: a team's project has the Mine filter, a personal one none. An
 * older control sends no `owner`, and then `team` says. */
export function modeOf(p: Pick<PlatformProject, "id" | "name" | "team" | "owner">): DocsMode {
  const team = p.owner ? p.owner.kind === "team" : !!p.team;
  return team ? { kind: "team", project: p.id, name: p.name } : PERSONAL;
}

/** Made by `me`, or by one of their agents. */
export const madeBy = (d: Pick<ProjectDoc, "author">, me: string | null | undefined): boolean => !!me && d.author === me;
