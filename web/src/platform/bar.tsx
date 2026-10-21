// The platform page's bar (M150): where you are (the project, with a way
// to the others), the platform's places, and a way back to the shell on
// control's page, where the machines, the keys and the trust screens are.
// Built from the shell's bar's classes, so the two pages look alike.

import type { ComponentChildren } from "preact";
import { useSubscribe } from "../shared/ui/hooks";
import { openMenu, type MenuItem } from "../shared/ui/menu";
import { follow, go, pathOf, useRoute, type Route } from "./router";
import { controlOrigin, session } from "./session";
import { names } from "./links";

/** The project the route is in, else the reader's own. */
export function routeProject(r: Route): string | undefined {
  return ("project" in r ? r.project : undefined) ?? session.personal()?.id;
}

function Place({ to, here, children, data }: { to: string; here: boolean; children: ComponentChildren; data: string }) {
  return (
    <a class={here ? "place here" : "place"} href={to} aria-current={here ? "page" : undefined} {...{ [`data-platform-${data}`]: "" }} onClick={(e) => follow(e, to)}>
      {children}
    </a>
  );
}

export function PlatformBar({ children }: { children?: ComponentChildren }) {
  useSubscribe((fn) => session.subscribe(fn));
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
  return (
    <header class="bar app-bar platform-bar" data-tauri-drag-region data-platform-bar>
      <button class="scope-label platform-project" data-project-switcher title="Your projects" onClick={pick}>
        {p?.name ?? "Arugula"} <span class="caret">▾</span>
      </button>
      <nav class="places" aria-label="Places">
        {project && (
          <Place to={pathOf({ page: "project", project }, names)} here={r.page === "project" || r.page === "workspace"} data="home">
            Project
          </Place>
        )}
        {project && (
          <Place to={pathOf({ page: "chat", project }, names)} here={r.page === "chat" || r.page === "dm"} data="chat">
            Chat
          </Place>
        )}
        <Place to="/agents" here={r.page === "agents"} data="agents">
          Agents
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
        <span class="account-button platform-me" title={me.login ? `Signed in as ${me.login}` : "Signed in"} data-signed-in-as>
          <span class="account-avatar" aria-hidden="true">
            {(me.login || me.name || "?").slice(0, 1).toUpperCase()}
          </span>
          <span class="account-login">{me.login || me.name}</span>
        </span>
      )}
    </header>
  );
}
