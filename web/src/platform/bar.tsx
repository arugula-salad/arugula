// The platform page's bar (M150): where you are (the project, with a way
// to the others), the platform's places, and a way back to the shell on
// control's page, where the machines, the keys and the trust screens are.
// It's the shell's workspace head (shared/frame.css, #1018): the wordmark
// at its left end, then the project and its places, so the two pages sit
// in one frame.

import type { ComponentChildren } from "preact";
import { useSubscribe } from "../shared/ui/hooks";
import { openMenu, type MenuItem } from "../shared/ui/menu";
import { openAppearance } from "../shared/ui/appearance";
import { WindowButtons } from "../shared/ui/window-buttons";
import { follow, go, pathOf, useRoute, type Route } from "./router";
import { controlOrigin, session } from "./session";
import { signOut } from "./signin";
import { names } from "./links";
import { chat } from "../shared/chat-store";
import { AccountPicture } from "../shared/chat/avatar";
import { presenceItems } from "../shared/chat/presence";
import { PlaceUnread } from "./thread-unread";

/** The project the route is in, else the reader's own. */
export function routeProject(r: Route): string | undefined {
  return ("project" in r ? r.project : undefined) ?? session.personal()?.id;
}

function Place({ to, here, children, data }: { to: string; here: boolean; children: ComponentChildren; data: string }) {
  return (
    <a class={here ? "place frame-place here" : "place frame-place"} href={to} aria-current={here ? "page" : undefined} {...{ [`data-platform-${data}`]: "" }} onClick={(e) => follow(e, to)}>
      {children}
    </a>
  );
}

export function PlatformBar({ children }: { children?: ComponentChildren }) {
  useSubscribe((fn) => session.subscribe(fn));
  // Issues come with chat (M201): its Labs flag says whether they're there,
  // and its rooms what's new in issues' and runs' threads.
  useSubscribe((fn) => chat.subscribe(fn));
  const r = useRoute();
  const project = routeProject(r);
  const p = session.project(project);
  const pick = (e: MouseEvent) => {
    const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const items: MenuItem[] = [
      { header: "Projects" },
      ...session.projects.map((x) => ({ label: x.name, checked: x.id === project, run: () => go(pathOf({ page: "project", project: x.id }, names)) })),
    ];
    openMenu({ clientX: at.left, clientY: at.bottom + 4, preventDefault: () => e.preventDefault() }, items);
  };
  const me = session.me;
  const account = (e: MouseEvent) => {
    const at = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const items: MenuItem[] = [
      { header: me?.login || me?.name || "Signed in" },
      { label: "Settings", run: () => go("/settings") },
      // This device's look, as in the shell's settings menu; on arugula.io
      // the two pages share it.
      { label: "Appearance…", run: openAppearance },
      // Who's around (#110): chat's people see this, and only they.
      ...presenceItems(),
      { label: "Your machines, devices and teams ↗", run: () => location.assign(controlOrigin()) },
      "separator",
      // This page only: the hub's session, on control's page, stays.
      { label: "Sign out of this page", run: () => void signOut().then(() => location.reload()) },
    ];
    openMenu({ clientX: at.right, clientY: at.bottom + 4, preventDefault: () => e.preventDefault() }, items);
  };
  return (
    <header class="ws-head platform-bar" data-tauri-drag-region data-platform-bar>
      {/* As the shell's "Arugula / Control": which page this is, and the
          way to its overview, every project. */}
      <a
        class="ws-wordmark"
        href="/"
        aria-label="Arugula Projects: all your projects"
        title="All your projects"
        aria-current={r.page === "home" ? "page" : undefined}
        data-platform-wordmark
        onClick={(e) => follow(e, "/")}
      >
        Arugula <span aria-hidden="true">/</span> <strong>Projects</strong>
      </a>
      <button class="scope-label platform-project" data-project-switcher title="Your projects" onClick={pick}>
        {p?.name ?? "Arugula"} <span class="caret">▾</span>
      </button>
      <nav class="places" aria-label="Places">
        {project && (
          <Place to={pathOf({ page: "project", project }, names)} here={r.page === "project" || r.page === "workspaces" || r.page === "workspace"} data="home">
            Project
          </Place>
        )}
        {project && chat.on && (
          <Place to={pathOf({ page: "issues", project }, names)} here={r.page === "issues" || r.page === "issue" || r.page === "new-issue"} data="issues">
            Issues
            <PlaceUnread project={project} prefix="iss_" />
          </Place>
        )}
        {project && (
          <Place to={pathOf({ page: "runs", project }, names)} here={r.page === "runs" || r.page === "run" || r.page === "session"} data="runs">
            Runs
            <PlaceUnread project={project} prefix="run_" />
          </Place>
        )}
        {project && (
          <Place to={pathOf({ page: "chat", project }, names)} here={r.page === "chat" || r.page === "dm"} data="chat">
            Chat
          </Place>
        )}
        <Place to="/inbox" here={r.page === "inbox"} data="inbox">
          Inbox
        </Place>
        <Place to="/settings" here={r.page === "settings"} data="settings">
          Settings
        </Place>
      </nav>
      {children}
      <div class="bar-fill" data-tauri-drag-region />
      <a class="place platform-shell" href={controlOrigin()} title="Your machines, terminals and devices, on control's page" data-platform-shell>
        Machines ↗
      </a>
      {me && (
        <button class="account-button platform-me" title={me.login ? `Signed in as ${me.login}` : "Signed in"} aria-label={`Account: ${me.login || me.name}`} data-signed-in-as onClick={account}>
          <span class="account-avatar" aria-hidden="true">
            {(me.login || me.name || "?").slice(0, 1).toUpperCase()}
            <AccountPicture />
          </span>
          <span class="account-login">{me.login || me.name}</span> <span class="caret">▾</span>
        </button>
      )}
      {/* In the desktop app on Linux the bar is the titlebar (M157). */}
      <WindowButtons />
    </header>
  );
}
