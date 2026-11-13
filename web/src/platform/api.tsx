// The platform page's `PlatformApi` (M150): what chat reaches this page
// through. It has no machines: a pane's chip shows its quote and says where
// the pane is, and opening one is the shell's (a link to control's page).
// Its links are paths ("Platform URLs are ids with cosmetic slugs").

import { noMachines, type PlatformApi, type PlatformTeam } from "../shared/platform-api";
import { PlatformBar } from "./bar";
import { chatGo, leaveChatTo, swipeDown, swipeGo, swipeUp } from "./chat-nav";
import { chatProject, currentProject, lastTeamProject } from "./current-project";
import { names, roomPath } from "./links";
import { current, go, pathOf, type Route } from "./router";
import { controlOrigin, session } from "./session";
import { toast } from "./toast";
import { barProject } from "./where";

const chatIn = (view?: "activity" | "saved" | "search" | "threads", q?: string, all?: boolean): Route | null => {
  // The project the bar is in (each has its own rooms, UI audit #169),
  // else a team's: the current one's (Arugula #882).
  const project = barProject(current()) ?? chatProject(session.projects, currentProject(), lastTeamProject()) ?? session.personal()?.id;
  return project ? { page: "chat", project, view, q, all } : null;
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
      const project = barProject(current());
      if (project) leaveChatTo(project);
      else go("/");
    },
    openRoom: (room, msg, thread) => chatGo(roomPath(room, msg, thread)),
    up: () => swipeUp(),
    down: () => swipeDown(),
    goTo: swipeGo,
    openChatView(view, q = "", all = false) {
      const r = chatIn(view, q, all);
      if (!r) return;
      // Typing in search replaces the entry, so Back doesn't step through it.
      const now = current();
      go(pathOf(r, names), view === "search" && now.page === "chat" && now.view === "search");
    },
    roomLink: (room, seq, thread) => `${location.origin}${roomPath(room, seq, thread)}`,
    roomHref: (room) => roomPath(room),
    openGrants: () => go("/settings"),
  },
  ...noMachines,
  // A pane's chip opens on control's page, where the machines are.
  goToPane: () => toast(`Panes open on your machines (${new URL(controlOrigin()).host})`),
  goToSession: () => toast(`Sessions open on your machines (${new URL(controlOrigin()).host})`),
  Bar: PlatformBar,
  controlHref: (hash) => `${controlOrigin()}/#${hash}`,
  sessionHref: (project, ses) => pathOf({ page: "session", project, session: ses }, names),
};
