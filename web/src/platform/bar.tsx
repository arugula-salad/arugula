// The app's bar (M150; the zoom path of Arugula #989): where you are, as a
// path you can climb (the team, the project, the place). It's the shell's
// workspace head (shared/frame.css, #1018), so the two pages sit in one
// frame. The places are the rail's (./rail). On a phone, with no rail, the
// bar has the project's switcher, the Inbox's bell and you, and the places
// are a bar along the bottom.

import type { ComponentChildren } from "preact";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { useRef, useState } from "preact/hooks";
import { openMenu, pressedShut, useMenuFrom, type MenuItem } from "../shared/ui/menu";
import { WindowButtons } from "../shared/ui/window-buttons";
import { currentProject } from "./current-project";
import { follow, go, pathOf, useRoute, type Route } from "./router";
import { session } from "./session";
import { signOut } from "./signin";
import { names } from "./links";
import { AccountPicture } from "../shared/chat/avatar";
import { presenceItems } from "../shared/chat/presence";
import { barProject, where } from "./where";
import { scopable, scopeOf } from "./scope";
import { useInboxCount } from "./inbox-count";
import { BellIcon, Caret, ChevronIcon } from "../shared/ui/icons";
import { ownerKey, ownerNamed } from "./owners";
import { Switcher } from "./switcher";
import { boards } from "../shared/boards/store";

/** The bar's place a route is under (UI audit #159): a project's cloud
 * workspaces and its deploys (Deploy… and a deploy) have places of their
 * own (Discovery 5). */
type Section = "home" | "boards" | "docs" | "runs" | "sessions" | "workspaces" | "deploys" | "chat" | "inbox" | "settings";
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
    case "sessions":
    case "session":
      return "sessions";
    case "chat":
    case "dm":
      return "chat";
    case "inbox":
      return "inbox";
    case "settings":
    case "owner-settings":
    case "project-settings":
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
const SECTION_LABEL: Record<Section, string> = {
  home: "Overview",
  boards: "Boards",
  docs: "Docs",
  runs: "Runs",
  sessions: "Sessions",
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
    case "sessions":
    case "workspaces":
    case "deploys":
      return pathOf({ page: s, project }, names);
    case "chat":
      return chatPath(project);
    case "settings":
      return pathOf({ page: "project-settings", project }, names);
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
    crumbs.push({ label: p.team?.name ?? "Personal", data: "owner" });
    crumbs.push({ label: p.name, to: pathOf({ page: "project", project }, names), data: "project" });
    if (r.page === "project" && r.board && r.card) {
      // A card open on the Overview: each level a step up (the prototype's zoom path).
      crumbs.push({ label: SECTION_LABEL.home, to: pathOf({ page: "project", project }, names), data: "home" });
      crumbs.push({ label: boards.title(r.board) ?? "Board", to: pathOf({ page: "board", project, board: r.board }, names), data: "board" });
      crumbs.push({ label: where.cardLabel(r.card) ?? "Card", data: "card" });
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

/** You, and your account's menu: Settings, who's around, signing out. On
 * the rail's foot (`compact`, the picture alone) or a phone's bar. */
export function AccountButton({ compact = false }: { compact?: boolean }) {
  useSubscribe((fn) => session.subscribe(fn));
  const button = useRef<HTMLButtonElement>(null);
  const open = useMenuFrom(button);
  const me = session.me;
  if (!me) return null;
  const account = (e: MouseEvent) => {
    const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const items: MenuItem[] = [
      { header: me.login || me.name || "Signed in", name: !!(me.login || me.name) },
      { label: "Settings", run: () => go("/settings") },
      // Who's around (#110): chat's people see this, and only they.
      ...presenceItems(),
      "separator",
      // This page only: the hub's session, on control's page, stays.
      { label: "Sign out of this page", run: () => void signOut().then(() => location.reload()) },
    ];
    openMenu({ clientX: compact ? at.right + 4 : at.right, clientY: compact ? at.top : at.bottom + 4, preventDefault: () => e.preventDefault() }, items, e.currentTarget as HTMLElement);
  };
  return (
    <button
      ref={button}
      class={compact ? "account-button platform-me ws-rail-me" : "account-button platform-me"}
      title={me.login ? `Signed in as ${me.login}` : "Signed in"}
      aria-label={compact ? `You: ${me.login || me.name}` : undefined}
      aria-haspopup="menu"
      aria-expanded={open}
      data-signed-in-as
      onClick={account}
    >
      <span class="account-avatar" aria-hidden="true">
        {(me.login || me.name || "?").slice(0, 1).toUpperCase()}
        <AccountPicture />
      </span>
      {!compact && (
        <>
          <span class="account-login">{me.login || me.name}</span> <Caret />
        </>
      )}
    </button>
  );
}

export function PlatformBar({ children }: { children?: ComponentChildren }) {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => where.subscribe(fn));
  const r = useRoute();
  // A phone has no rail (#377): the bar keeps the switcher and the bell.
  const rail = !usePhone();
  // The bar keeps the current project while Boards, Docs and the Inbox are scoped to it (Arugula #881).
  const project = barProject(r) ?? (scopable(r) ? scopeOf(r, currentProject(), (id) => !!session.project(id)) : undefined);
  const p = session.project(project);
  return (
    <header class="ws-head platform-bar" data-tauri-drag-region data-platform-bar>
      {rail ? <Path r={r} project={project} /> : <PhoneTop r={r} project={project} name={project ? (p?.name ?? "…") : "All projects"} />}
      {children}
      <div class="bar-fill" data-tauri-drag-region />
      {!rail && <Bell here={r.page === "inbox"} />}
      {!rail && <AccountButton compact />}
      {/* In the desktop app on Linux the bar is the titlebar (M157). */}
      <WindowButtons />
    </header>
  );
}

/** A phone's bar: the project's name, which opens the switcher. */
function PhoneTop({ r, project, name }: { r: Route; project: string | undefined; name: string }) {
  const [picking, setPicking] = useState(false);
  const switcher = useRef<HTMLButtonElement>(null);
  const team = r.page === "team" ? ownerNamed(session.projects, r.team) : undefined;
  const p = session.project(project);
  return (
    <>
      <button
        ref={switcher}
        class="scope-label platform-project"
        data-project-switcher
        title="Your projects"
        aria-label={`Project: ${name}`}
        aria-haspopup="dialog"
        aria-expanded={picking}
        onClick={() => pressedShut(switcher.current) || setPicking(!picking)}
      >
        <span class="platform-project-name">{name}</span> <Caret />
      </button>
      {picking && <Switcher from={switcher} project={project} owner={team?.key ?? (p ? ownerKey(p) : undefined)} close={() => setPicking(false)} />}
    </>
  );
}

/** The phone's Inbox: the bell, with its count. */
function Bell({ here }: { here: boolean }) {
  const { n, waiting, title } = useInboxCount();
  return (
    <a class={here ? "app-bell here" : "app-bell"} href="/inbox" title={title || "Inbox"} aria-label={n ? `Inbox, ${n}` : "Inbox"} aria-current={here ? "page" : undefined} data-phone-bell onClick={(e) => follow(e, "/inbox")}>
      <BellIcon />
      {n > 0 && <span class={waiting ? "ws-rail-count mention" : "ws-rail-count"}>{n > 99 ? "99+" : n}</span>}
    </a>
  );
}
