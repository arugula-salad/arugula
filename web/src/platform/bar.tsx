// The app's bar (M150; the zoom path of Arugula #989): where you are, as a
// path you can climb (the team, the project, the place). It's the shell's
// workspace head (shared/frame.css, #1018), so the two pages sit in one
// frame. The place you're in's views follow it (./rail's PlaceViews). On a
// phone, with no rail, the bar has the project's switcher, those views, the
// Inbox's bell and you, and the places are a bar along the bottom.

import type { ComponentChildren } from "preact";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { useRef } from "preact/hooks";
import { openMenu, useMenuFrom, type MenuItem } from "../shared/ui/menu";
import { WindowButtons } from "../shared/ui/window-buttons";
import { currentProject } from "./current-project";
import { follow, go, pathOf, useRoute, type Route } from "./router";
import { controlOrigin, session } from "./session";
import { openStatusPage, statusPage } from "./status-page";
import { signOut } from "./signin";
import { openPlatformFeedback } from "./feedback";
import { names } from "./links";
import { Avatar } from "../shared/kit/avatar";
import { barProject, where } from "./where";
import { scopable, scopeOf } from "./scope";
import { Caret, ChevronIcon } from "../shared/ui/icons";
import { openGoTo } from "../shared/kit/frame/goto";
import { boards } from "../shared/boards/store";
import { PhoneInbox } from "./rail";

/** The bar's place a route is under (UI audit #159): a project's cloud
 * workspaces and its deploys (Deploy… and a deploy) have places of their
 * own (Discovery 5). */
type Section = "home" | "boards" | "docs" | "runs" | "workspaces" | "deploys" | "chat" | "inbox" | "settings";
export function sectionOf(r: Route): Section | null {
  switch (r.page) {
    case "project":
      return "home";
    case "workspaces":
    case "workspace":
      return "workspaces";
    case "deploys":
    case "new-deploy":
    case "deploy":
      return "deploys";
    // Issues and Triage are part of Boards (#656): the one place for tickets.
    case "issues":
    case "issue":
    case "new-issue":
    case "triage":
    // The Sessions page folded into the boards' rows (Arugula #1082); a
    // session's page is a card's.
    case "sessions":
    case "session":
    case "boards":
    case "board":
    case "all-boards":
      return "boards";
    case "docs":
    case "doc":
      return "docs";
    case "runs":
    case "run":
      return "runs";
    case "chat":
    case "dm":
      return "chat";
    case "inbox":
      return "inbox";
    case "settings":
    case "move":
    case "installed":
      return "settings";
    default:
      return null;
  }
}

/** Each project's chat is its own rooms (UI audit #169): a team's
 * channels and DMs, Personal's your machines' threads. */
export function chatPath(project: string | undefined): string {
  return project ? pathOf({ page: "chat", project }, names) : "/chat";
}

/** The words for a place, in the path. */
export const SECTION_LABEL: Record<Section, string> = {
  home: "Overview",
  boards: "Boards",
  docs: "Docs",
  runs: "Runs",
  workspaces: "Workspaces",
  deploys: "Deploys",
  chat: "Chat",
  inbox: "Inbox",
  settings: "Settings",
};

/** A place's own page, in the project, for the path's link back to it. */
function sectionPath(s: Section, project: string): string | null {
  switch (s) {
    case "home":
      return pathOf({ page: "project", project }, names);
    case "boards":
    case "docs":
    case "runs":
    case "workspaces":
    case "deploys":
      return pathOf({ page: s, project }, names);
    case "chat":
      return chatPath(project);
    case "settings":
      return pathOf({ page: "settings", scope: "project", project }, names);
    default:
      return null;
  }
}

/** Where you are, as a path you can climb (Arugula #989, the prototype's
 * zoom path): the team, the project, the place; each a level up. */
function Path({ r, project }: { r: Route; project: string | undefined }) {
  const p = session.project(project);
  const section = sectionOf(r);
  const crumbs: { label: string; to?: string; data: string }[] = [];
  if (project && p) {
    crumbs.push({ label: p.team?.name ?? "Just you", data: "owner" });
    crumbs.push({ label: p.name, to: pathOf({ page: "project", project }, names), data: "project" });
    if (r.page === "project" && r.board && r.card) {
      // A card open on the Overview: each level a step up (the prototype's zoom path).
      crumbs.push({ label: SECTION_LABEL.home, to: pathOf({ page: "project", project }, names), data: "home" });
      crumbs.push({ label: boards.title(r.board) ?? "Board", to: pathOf({ page: "board", project, board: r.board }, names), data: "board" });
      crumbs.push({ label: where.cardLabel(r.card) ?? "Card", data: "card" });
    } else if (r.page === "project" && r.zoom) {
      // The Overview zoomed in (Arugula #1287): Overview › Board › #N title, each a level up.
      const z = r.zoom;
      crumbs.push({ label: SECTION_LABEL.home, to: pathOf({ page: "project", project }, names), data: "home" });
      if ("board" in z) crumbs.push({ label: boards.title(z.board) ?? "Board", to: pathOf({ page: "project", project, zoom: { board: z.board } }, names), data: "board" });
      if ("board" in z && z.card) crumbs.push({ label: where.cardLabel(z.card) ?? "Card", data: "card" });
      if ("session" in z) crumbs.push({ label: where.cardLabel(z.session) ?? "Session", data: "session" });
    } else if (r.page === "machines" && r.at.at === "swarm") {
      // Swarm is the Overview's view (Arugula #1348): Overview › Swarm.
      crumbs.push({ label: SECTION_LABEL.home, to: pathOf({ page: "project", project }, names), data: "home" });
      crumbs.push({ label: "Swarm", data: "swarm" });
    } else if (r.page === "issues" || r.page === "issue" || r.page === "new-issue") {
      // The issues are Boards' (#656): Boards › Issues, each a level up (Arugula #1387).
      crumbs.push({ label: SECTION_LABEL.boards, to: sectionPath("boards", project) ?? undefined, data: "boards" });
      crumbs.push({ label: "Issues", to: pathOf({ page: "issues", project }, names), data: "issues" });
    } else if (r.page === "machines") crumbs.push({ label: "Machines", to: pathOf({ page: "machines", project, at: { at: "machines" } }, names), data: "machines" });
    else if (section && section !== "home") crumbs.push({ label: SECTION_LABEL[section], to: sectionPath(section, project) ?? undefined, data: section });
  } else {
    crumbs.push({ label: project ? (session.state === "ready" ? "Unknown project" : "…") : "All projects", to: project ? undefined : "/", data: "all" });
    if (section && section !== "home") crumbs.push({ label: SECTION_LABEL[section], data: section });
  }
  const here = location.pathname;
  return (
    <nav class="ws-crumbs app-path" aria-label="Where you are" data-app-path data-tauri-drag-region>
      {crumbs.map((c, i) => {
        const last = i === crumbs.length - 1;
        return (
          <span key={c.data} class="app-path-step">
            {i > 0 && (
              <span class="app-path-sep" aria-hidden="true">
                <ChevronIcon dir="right" />
              </span>
            )}
            {c.to && !(last && c.to.split("?")[0] === here) ? (
              <a href={c.to} data-path={c.data} onClick={(e) => follow(e, c.to!)}>
                {c.label}
              </a>
            ) : (
              <span class={last ? "app-path-here" : "dim"} aria-current={last ? "page" : undefined} data-path={c.data}>
                {c.label}
              </span>
            )}
          </span>
        );
      })}
    </nav>
  );
}

/** You, and your account's menu: the status page on hosted Arugula
 * (Arugula #1176), feedback and signing out; your presence is Chat's
 * (Arugula #1079). Your initials (the kit's Avatar), on the rail's foot or a
 * phone's bar. The rail has Settings, so the menu has no way there; a phone
 * has no rail, so its menu keeps Your account… (Arugula #1319). */
export function AccountButton() {
  useSubscribe((fn) => session.subscribe(fn));
  const button = useRef<HTMLButtonElement>(null);
  const open = useMenuFrom(button);
  const phone = usePhone();
  const me = session.me;
  if (!me) return null;
  const name = me.login || me.name || "?";
  const account = (e: MouseEvent) => {
    const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const status = statusPage(controlOrigin());
    const items: MenuItem[] = [
      { header: me.login || me.name || "Signed in", name: !!(me.login || me.name) },
      ...(phone ? [{ label: "Your account…", run: () => go("/settings") }] : []),
      ...(status ? [{ label: "Status", external: true, hint: "Whether hosted Arugula is up", attrs: { "data-status-page": "" }, run: () => openStatusPage(status) }] : []),
      // Arugula #1240: straight to control, as the person signed in.
      { label: "Send feedback…", run: openPlatformFeedback },
      "separator",
      // This page only: the hub's session, on control's page, stays.
      { label: "Sign out of this page", run: () => void signOut().then(() => location.reload()) },
    ];
    openMenu({ clientX: phone ? at.right : at.right + 4, clientY: phone ? at.bottom + 4 : at.top, preventDefault: () => e.preventDefault() }, items, e.currentTarget as HTMLElement);
  };
  return (
    <button
      ref={button}
      type="button"
      class="k-rail-me"
      title={me.login ? `Signed in as ${me.login}` : "Signed in"}
      aria-haspopup="menu"
      aria-expanded={open}
      data-account-menu
      data-signed-in-as
      onClick={account}
    >
      <Avatar name={name} />
    </button>
  );
}

export function PlatformBar({ children }: { children?: ComponentChildren }) {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => where.subscribe(fn));
  const r = useRoute();
  // A phone has no rail (#377): the bar keeps the switcher and the Inbox's bell (Arugula #1345).
  const rail = !usePhone();
  // The bar keeps the current project while Boards, Docs and the Inbox are scoped to it (Arugula #881).
  const project = barProject(r) ?? (scopable(r) ? scopeOf(r, currentProject(), (id) => !!session.project(id)) : undefined);
  const p = session.project(project);
  return (
    <header class="ws-head platform-bar" data-tauri-drag-region data-platform-bar>
      {rail ? <Path r={r} project={project} /> : <PhoneTop name={project ? (p?.name ?? "…") : "All projects"} />}
      {children}
      <div class="bar-fill" data-tauri-drag-region />
      {!rail && <PhoneInbox />}
      {!rail && <AccountButton />}
      {/* In the desktop app on Linux the bar is the titlebar (M157). */}
      <WindowButtons />
    </header>
  );
}

/** A phone's bar: the project's name, which opens Go to…'s projects. */
function PhoneTop({ name }: { name: string }) {
  return (
    <button class="scope-label platform-project" data-project-switcher title="Your projects" aria-label={`Project: ${name}`} aria-haspopup="dialog" onClick={() => openGoTo("projects")}>
      <span class="platform-project-name">{name}</span> <Caret />
    </button>
  );
}
