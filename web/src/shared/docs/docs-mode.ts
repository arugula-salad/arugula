// Which Docs the one Docs list shows (Arugula #864): a person's own, or a
// team's in two tabs, never the two mixed. Plain functions over what the
// hosts know, tested in scripts/docs-mode.test.mjs.
//
// - Personal: "My Docs", no tabs. On a machine that's its library; on the
//   platform, a personal project's Docs.
// - A team: My Docs and Team Docs, both the team project's. My Docs is what
//   the reader made, their agents' included: a machine's agent makes a Doc
//   as its owner (`author`), with the machine as `agent`.
//
// A machine is in a team when it's a team's machine (its enrollment's team,
// as `GET /api/access` says it), the rule `doc new` goes by (Arugula #862);
// its Docs are then the team's default project's.

import type { PlatformProject, ProjectDoc } from "../proto.gen.ts";

export type DocsMode = { kind: "personal" } | { kind: "team"; project: string; name: string };
export type DocsTab = "mine" | "team";

export const PERSONAL: DocsMode = { kind: "personal" };

/** A project's mode: a team's project has the tabs, a personal one none. An
 * older control sends no `owner`, and then `team` says. */
export function modeOf(p: Pick<PlatformProject, "id" | "name" | "team" | "owner">): DocsMode {
  const team = p.owner ? p.owner.kind === "team" : !!p.team;
  return team ? { kind: "team", project: p.id, name: p.name } : PERSONAL;
}

/** A machine's mode: its team's default project's, when it's a team's
 * machine and the reader has that project; else its own library. */
export function machineMode(team: string | null | undefined, projects: readonly Pick<PlatformProject, "id" | "name" | "team" | "owner">[]): DocsMode {
  if (!team) return PERSONAL;
  const p = projects.find((x) => x.team?.id === team);
  return p ? { kind: "team", project: p.id, name: p.name } : PERSONAL;
}

/** Made by `me`, or by one of their agents. */
export const madeBy = (d: Pick<ProjectDoc, "author">, me: string | null | undefined): boolean => !!me && d.author === me;

/** Whether a Doc shows in `tab`: My Docs is the reader's own, Team Docs all. */
export const inTab = (tab: DocsTab, mine: boolean): boolean => tab === "team" || mine;

const TAB_KEY = "docs-tab";

type Store = Pick<Storage, "getItem" | "setItem">;
const local = (): Store | null => (typeof localStorage === "undefined" ? null : localStorage);

/** The tab this browser used last: My Docs until someone lives in Team Docs. */
export function savedTab(store: Store | null = local()): DocsTab {
  try {
    return store?.getItem(TAB_KEY) === "team" ? "team" : "mine";
  } catch {
    return "mine";
  }
}

export function saveTab(tab: DocsTab, store: Store | null = local()) {
  try {
    store?.setItem(TAB_KEY, tab);
  } catch {
    /* It's only remembered while storage works. */
  }
}
