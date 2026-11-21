// Settings' scopes (Arugula #1104), the one list of them: You,
// Machines, Project, and Team and billing. Each says its name, whose its
// settings are, what its picker chooses (a scope of many), and draws its
// sections: every one of them, each a `Section` (shared/ui/settings-section)
// with an id, a title, a summary and its controls. The page
// (../pages/settings.tsx) lists them and shows the one the address names.
//
// A new section goes in its scope's component; a new scope, here and in
// `SettingsRoute` (../paths.ts).

import type { ComponentChildren } from "preact";
import { currentProject } from "../current-project";
import { owners, ownerKey } from "../owners";
import type { SettingsRoute, SettingsScopeKey } from "../paths";
import { session } from "../session";
import { MachineSettings } from "../pages/machine-settings";
import { OwnerSettings } from "../pages/owner-settings";
import { ProjectSettings } from "../pages/project-settings";
import { YouSettings } from "../pages/you-settings";

type RouteOf<K extends SettingsScopeKey> = Extract<SettingsRoute, { scope: K }>;

export interface SettingsScope<K extends SettingsScopeKey = SettingsScopeKey> {
  key: K;
  /** Its name, in the scopes' switch. */
  title: string;
  /** Whose these are, under the header. */
  whose: string;
  /** What its picker chooses, for a scope of many. */
  pick?: string;
  /** All of its sections. */
  sections(r: RouteOf<K>): ComponentChildren;
}

export const SCOPES: { [K in SettingsScopeKey]: SettingsScope<K> } = {
  you: {
    key: "you",
    title: "You",
    whose: "Yours, wherever you sign in. Appearance is this device's.",
    sections: () => <YouSettings />,
  },
  machine: {
    key: "machine",
    title: "Machines",
    whose: "One of your machines.",
    pick: "Machine",
    sections: (r) => <MachineSettings machine={r.machine} section={r.section} />,
  },
  project: {
    key: "project",
    title: "Project",
    whose: "One project. Its members see these; its owners and editors change them.",
    pick: "Project",
    sections: (r) => <ProjectSettings id={r.project} link={r.link} />,
  },
  team: {
    key: "team",
    title: "Team and billing",
    whose: "A team, or just you: who's in it, what it pays, and what its projects start with.",
    pick: "Team",
    sections: (r) => <OwnerSettings owner={r.owner} />,
  },
};

/** The scopes, in the switch's order. */
export const SCOPE_KEYS: SettingsScopeKey[] = ["you", "machine", "project", "team"];

/** Where a scope opens from the switch: the project you're in (else your
 * first), and its owner; a machine the page picks once it knows them.
 * Null when there's nothing to open (no project yet). */
export function scopeRoute(key: SettingsScopeKey): SettingsRoute | null {
  const live = session.projects.filter((p) => !p.archived);
  const here = session.project(currentProject()) ?? live[0] ?? session.projects[0];
  switch (key) {
    case "you":
      return { page: "settings", scope: "you" };
    case "machine":
      return { page: "settings", scope: "machine" };
    case "project":
      return here ? { page: "settings", scope: "project", project: here.id } : null;
    case "team":
      return { page: "settings", scope: "team", owner: here ? ownerKey(here) : "personal" };
  }
}

/** What a scope's picker offers: the projects that aren't archived (and
 * the one shown, if it is), or Personal and each team. */
export function choicesOf(r: SettingsRoute): { id: string; name: string }[] {
  if (r.scope === "project") {
    const shown = session.project(r.project);
    const live = session.projects.filter((p) => !p.archived).sort((a, b) => a.name.localeCompare(b.name));
    return [...(shown?.archived ? [shown] : []), ...live].map((p) => ({ id: p.id, name: p.name }));
  }
  if (r.scope === "team") return owners(session.projects).map((o) => ({ id: o.key, name: o.name }));
  return [];
}
