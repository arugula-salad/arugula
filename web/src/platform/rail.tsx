// The app's rail (#377; the project-rooted rail of Arugula #989): the same
// one as the panes page's (shared/ui/rail), down the left on a computer.
// The project tile at the top is the switcher. Then the project's places
// (Overview, Boards, Sessions, Docs, Deploys), a line, the team's (Chat,
// Machines), and at the foot the Inbox, Settings and you. The bar beside
// it (./bar) is the path to where you are. A phone has no rail: the same
// places are a bar along the bottom (`PhoneTabs`).

import type { ComponentChildren } from "preact";
import { useRef, useState } from "preact/hooks";
import { chat } from "../shared/chat-store";
import { chatUnread, projectUnread } from "../shared/chat/unread";
import { usePlatformApi } from "../shared/platform-api";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { BellIcon, BoardsIcon, ChatIcon, DeploysIcon, DocsIcon, MachineIcon, MoreIcon, ProjectsIcon, SessionsIcon, SettingsIcon } from "../shared/ui/icons";
import { openMenu, pressedShut, type MenuItem } from "../shared/ui/menu";
import { Rail, RAIL_LINE, type RailItem } from "../shared/ui/rail";
import { AccountButton, chatPath, sectionOf } from "./bar";
import { chatTeam } from "./chat-scope";
import { currentProject, entryPath, onCurrentProject } from "./current-project";
import { useInboxCount } from "./inbox-count";
import { names } from "./links";
import { ownerKey } from "./owners";
import { go, pathOf, useRoute, type Route } from "./router";
import { controlOrigin, session } from "./session";
import { Switcher } from "./switcher";
import { paneOf } from "./places-kept";
import { subscribeSeen } from "../shared/seen";
import { barProject, where } from "./where";

/** A place on the rail or the phone's bar. */
interface PlaceLink {
  key: string;
  label: string;
  tip: string;
  icon: ComponentChildren;
  to: string;
  here: boolean;
  count?: { n: number; mention: boolean };
}

/** The places, for the project the page is in (else the current one, else
 * Personal's), as both the rail and the phone's bar draw them. */
function usePlaces() {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => chat.subscribe(fn));
  useSubscribe((fn) => where.subscribe(fn));
  useSubscribe(onCurrentProject);
  useSubscribe(subscribeSeen);
  const r = useRoute();
  const { Machines } = usePlatformApi();
  const inbox = useInboxCount();
  const shown = barProject(r);
  const p = session.project(shown);
  // As the bar does: an id that's none of the reader's lights no place.
  const unknown = !!shown && !p && session.state === "ready";
  const section = unknown ? null : sectionOf(r);
  const project = p ?? session.project(currentProject()) ?? session.personal();
  const at = (page: "project" | "boards" | "sessions" | "docs" | "deploys" | "project-settings") => (project ? pathOf({ page, project: project.id } as Route, names) : "/");
  const { n, mention } = !unknown ? chatUnread(chat.rooms, chatTeam(p, project)) : { n: 0, mention: false };
  const chatTo = shown ? chatPath(shown) : entryPath("chat", project, names);
  // The machines are the app's own places where it has them; else control's page.
  // Machines goes back to your last pane in this project (decision 5); the list is the path's step up.
  const machinesTo = Machines && project ? (paneOf(project.id) ?? pathOf({ page: "machines", project: project.id, at: { at: "machines" } }, names)) : `${controlOrigin()}/`;
  const machinesHere = r.page === "machines" || section === "workspaces";
  // What's new in the threads on the project's issues (Boards) and runs (Sessions, which holds Runs).
  const none = { n: 0, mention: false };
  const issues = !unknown && project && chat.on !== false ? projectUnread(chat.rooms, project.id, "iss_") : none;
  const runs = !unknown && project ? projectUnread(chat.rooms, project.id, "run_") : none;
  const own: PlaceLink[] = [
    { key: "overview", label: "Overview", tip: "Overview: this project's work", icon: <ProjectsIcon />, to: at("project"), here: section === "home" },
    { key: "boards", label: "Boards", tip: "Boards: this project's boards and tickets", icon: <BoardsIcon />, to: at("boards"), here: section === "boards" && !!shown, count: issues },
    // A run is a session in a cloud workspace (decision 6): Runs is under Sessions.
    { key: "sessions", label: "Sessions", tip: "Sessions: this project's sessions and runs", icon: <SessionsIcon />, to: at("sessions"), here: section === "sessions" || section === "runs", count: runs },
    { key: "docs", label: "Docs", tip: "Docs: this project's Docs", icon: <DocsIcon />, to: at("docs"), here: section === "docs" },
    { key: "deploys", label: "Deploys", tip: "Deploys: this project's deploys", icon: <DeploysIcon />, to: at("deploys"), here: section === "deploys" },
  ];
  const team: PlaceLink[] = [
    { key: "chat", label: "Chat", tip: "Chat: your team's rooms", icon: <ChatIcon />, to: chatTo, here: section === "chat", count: { n, mention } },
    { key: "machines", label: "Machines", tip: "Machines: your team's computers and cloud workspaces", icon: <MachineIcon size="lg" />, to: machinesTo, here: machinesHere },
  ];
  const foot: PlaceLink[] = [
    { key: "inbox", label: "Inbox", tip: inbox.title || "Inbox: what wants you, across your projects", icon: <BellIcon />, to: "/inbox", here: section === "inbox", count: { n: inbox.n, mention: inbox.waiting > 0 } },
    // A project's settings; account settings are the avatar's menu.
    { key: "settings", label: "Settings", tip: "Settings: this project's", icon: <SettingsIcon />, to: shown ? at("project-settings") : "/settings", here: section === "settings" },
  ];
  return { own, team, foot, project, shown };
}

const railItem = (p: PlaceLink): RailItem => {
  const external = /^https?:/.test(p.to);
  return {
    label: p.label,
    tip: p.tip,
    aria: p.count?.n ? `${p.label}, ${p.count.n}${p.count.mention ? ", for you" : ""}` : undefined,
    here: p.here,
    hook: `data-open-${p.key}`,
    icon: p.icon,
    count: p.count,
    href: p.to,
    // Control's page is a page load; the rest are this page's.
    onClick: external ? undefined : () => go(p.to),
  };
};

export function PlatformRail() {
  const { own, team, foot, project, shown } = usePlaces();
  const brand = <ProjectTile project={project} shown={shown} />;
  return <Rail brand={brand} items={[...own.map(railItem), RAIL_LINE, ...team.map(railItem)]} bottom={foot.map(railItem)} foot={<AccountButton compact />} />;
}

/** The project tile at the top of the rail: the project you're in, and the
 * switcher to every other. */
function ProjectTile({ project, shown }: { project: ReturnType<typeof session.project>; shown: string | undefined }) {
  const [picking, setPicking] = useState(false);
  const tile = useRef<HTMLButtonElement>(null);
  const name = shown && project ? project.name : "All projects";
  const initial = shown && project ? (project.name.trim()[0] ?? "?").toUpperCase() : "∗";
  return (
    <>
      <button
        ref={tile}
        class="ws-brand ws-project-tile"
        data-project-switcher
        title={`${name}: switch project`}
        aria-label={`Project: ${name}`}
        aria-haspopup="dialog"
        aria-expanded={picking}
        // A press on it shut the switcher: this click is the same tap.
        onClick={() => pressedShut(tile.current) || setPicking(!picking)}
      >
        <span class="ws-project-initial" aria-hidden="true">
          {initial}
        </span>
      </button>
      {picking && <Switcher beside from={tile} project={shown} owner={project ? ownerKey(project) : undefined} close={() => setPicking(false)} />}
    </>
  );
}

/** The phone's bar along the bottom (decision 15): Overview, Boards,
 * Sessions, Chat, and More for the rest. */
export function PhoneTabs() {
  const { own, team, foot } = usePlaces();
  const by = (k: string) => [...own, ...team, ...foot].find((p) => p.key === k)!;
  const tabs = ["overview", "boards", "sessions", "chat"].map(by);
  const rest = ["docs", "deploys", "machines", "settings"].map(by);
  const more = (e: MouseEvent) => {
    const items: MenuItem[] = rest.map((p) => ({ label: p.label, current: p.here, run: () => (/^https?:/.test(p.to) ? location.assign(p.to) : go(p.to)) }));
    openMenu(e, items, e.currentTarget as HTMLElement);
  };
  return (
    <nav class="app-tabs" aria-label="Places" data-phone-tabs>
      {tabs.map((p) => (
        <a key={p.key} class={p.here ? "app-tab here" : "app-tab"} href={p.to} aria-current={p.here ? "page" : undefined} {...{ [`data-tab-${p.key}`]: "" }} onClick={(e) => (e.preventDefault(), go(p.to))}>
          {p.icon}
          <span>{p.label}</span>
          {!!p.count?.n && <span class={p.count.mention ? "ws-rail-count mention" : "ws-rail-count"}>{p.count.mention ? `@${p.count.n}` : p.count.n}</span>}
        </a>
      ))}
      <button class={rest.some((p) => p.here) ? "app-tab here" : "app-tab"} aria-haspopup="menu" data-tab-more onClick={more}>
        <MoreIcon />
        <span>More</span>
      </button>
    </nav>
  );
}

/** The page's body beside its rail: the rail on a computer; on a phone, the
 * bar along the bottom instead, unless `tabs` is false (a machine's page,
 * whose key bar has the bottom). */
export function WithRail({ children, tabs = true }: { children: ComponentChildren; tabs?: boolean }) {
  const phone = usePhone();
  return (
    <>
      {!phone && <PlatformRail />}
      <div class="ws-shell-body platform-body">{children}</div>
      {phone && tabs && <PhoneTabs />}
    </>
  );
}
