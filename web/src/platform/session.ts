// Who's reading the platform page, and their projects (M150): one read of
// the platform's `GET /api/projects`, which the page asks the platform
// alone. Then chat and the Cloud workspaces start as the shell starts them,
// for the same account. Someone without a session here is sent through the
// hub to get one (./signin, M152): one silent hop when they're signed in on
// the hub. A hop that just came back without one isn't tried again by
// itself, and the page says where to sign in.

import type { PlatformMe, PlatformProject, PlatformProjects } from "../shared/proto.gen";
import { chat } from "../shared/chat-store";
import { cloudPanes, cloudWorkspaces } from "../shared/cloud";
import { platformFetch, refusalOf } from "../shared/maintenance";
import { finishSignIn, signIn, triedJustNow } from "./signin";

export type { PlatformProject };

/** Where the hub says it is (`X-Arugula-Hub` on its answers here, M152). */
let hub: string | null = null;

/** The hub's page, where people sign in and where the shell is: as the
 * hub says on its answers here, else what the dev recipes say (`just
 * dev-platform`, where this page is Vite's), else this origin. Never a
 * guess from this page's host name. */
export function controlOrigin(): string {
  return hub || (import.meta.env.VITE_CONTROL_ORIGIN as string | undefined) || location.origin;
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
      await finishSignIn();
      const r = await platformFetch("/api/projects", { credentials: "same-origin" });
      const said = r.headers.get("x-arugula-hub");
      if (said && /^https?:\/\/[^/]+$/.test(said)) hub = said;
      const v = (await r.json().catch(() => ({}))) as PlatformProjects & { error?: string; reason?: string };
      if (r.status === 401) {
        // Off through the hub, unless that's just been tried.
        if (!triedJustNow() && (await signIn(controlOrigin()))) return;
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
