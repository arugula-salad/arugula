// The platform page's `PlatformApi` (M150): what chat reaches this page
// through. It has no machines: a pane's chip shows its quote and says where
// the pane is, and opening one is the shell's (a link to control's page).
// Its links are paths ("Platform URLs are ids with cosmetic slugs").

import { noMachines, type PlatformApi, type PlatformTeam } from "../shared/platform-api";
import { PlatformBar } from "./bar";
import { names, roomPath, routeProject } from "./links";
import { current, go, pathOf, type Route } from "./router";
import { controlOrigin, session } from "./session";
import { toast } from "./toast";

const chatIn = (view?: "activity" | "saved" | "search" | "threads", q?: string): Route | null => {
  const project = routeProject() ?? session.personal()?.id;
  return project ? { page: "chat", project, view, q } : null;
};

export const platformApi: PlatformApi = {
  teams: (): PlatformTeam[] =>
    session.projects.flatMap((p) => (p.team ? [{ team: p.team.id, role: p.team.role, roster: { name: p.team.name, members: p.team.members } }] : [])),
  subscribe: (fn) => session.subscribe(fn),
  toast,
  nav: {
    openChat() {
      const r = chatIn();
      go(r ? pathOf(r, names) : "/");
    },
    closeChat() {
      const project = routeProject();
      go(project ? pathOf({ page: "project", project }, names) : "/");
    },
    openRoom: (room, msg, thread) => go(roomPath(room, msg, thread)),
    openChatView(view, q = "") {
      const r = chatIn(view, q);
      if (!r) return;
      // Typing in search replaces the entry, so Back doesn't step through it.
      const now = current();
      go(pathOf(r, names), view === "search" && now.page === "chat" && now.view === "search");
    },
    roomLink: (room, seq, thread) => `${location.origin}${roomPath(room, seq, thread)}`,
    openGrants: () => go("/settings"),
  },
  ...noMachines,
  // A pane's chip opens on control's page, where the machines are.
  goToPane: () => toast(`Panes open on control's page (${new URL(controlOrigin()).host}), where your machines are`),
  goToSession: () => toast(`Sessions open on control's page (${new URL(controlOrigin()).host}), where your machines are`),
  Bar: PlatformBar,
  controlHref: (hash) => `${controlOrigin()}/#${hash}`,
  sessionHref: (project, ses) => pathOf({ page: "session", project, session: ses }, names),
};
