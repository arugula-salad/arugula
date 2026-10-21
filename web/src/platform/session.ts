// Who's reading the platform page, and their projects (M150): one read of
// the platform's `GET /api/projects`, which the page asks the platform
// alone. Then chat and the Cloud workspaces start as the shell starts them,
// for the same account. Until someone's signed in here the page says where
// to sign in: the hub's page (M152 makes it one silent hop).

import type { PlatformMe, PlatformProject, PlatformProjects } from "../shared/proto.gen";
import { chat } from "../shared/chat-store";
import { cloudPanes, cloudWorkspaces } from "../shared/cloud";
import { platformFetch, refusalOf } from "../shared/maintenance";

export type { PlatformProject };

/** The hub's page, where people sign in and where the shell is: `control.`
 * beside `app.`, else what the dev recipe says, else this origin. */
export function controlOrigin(): string {
  if (location.hostname.startsWith("app.")) return location.origin.replace("//app.", "//control.");
  return (import.meta.env.VITE_CONTROL_ORIGIN as string | undefined) || location.origin;
}

const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export const session = {
  /** Who's signed in, once the platform has said. */
  me: null as PlatformMe | null,
  projects: [] as PlatformProject[],
  /** null while it's asking; "signed-out" for no session; else an error. */
  state: null as null | "ready" | "signed-out" | { error: string },
  subscribe(fn: () => void) {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  },
  project(id: string | null | undefined): PlatformProject | undefined {
    return id ? this.projects.find((p) => p.id === id) : undefined;
  },
  /** The reader's own project. */
  personal(): PlatformProject | undefined {
    return this.projects.find((p) => !p.team);
  },
  /** The project a team's rooms belong to. */
  ofTeam(team: string | null | undefined): PlatformProject | undefined {
    return team ? this.projects.find((p) => p.team?.id === team) : undefined;
  },
  async load() {
    try {
      const r = await platformFetch("/api/projects", { credentials: "same-origin" });
      const v = (await r.json().catch(() => ({}))) as PlatformProjects & { error?: string; reason?: string };
      if (r.status === 401) {
        this.state = "signed-out";
        return changed();
      }
      if (!r.ok) throw new Error(refusalOf(r.status, v));
      const first = !this.me;
      this.me = v.me;
      this.projects = v.projects;
      this.state = "ready";
      if (first) {
        const roster = v.projects.flatMap((p) => p.team?.members.filter((m) => m.account === v.me.account).map((m) => m.name) ?? []);
        chat.start(v.me.account, [v.me.login, v.me.name, ...roster]);
        cloudPanes.connect(v.me.account);
        cloudWorkspaces.connect(v.me.account);
      }
    } catch (e) {
      this.state = { error: (e as Error).message };
    }
    changed();
  },
};
