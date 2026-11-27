// The app's rail (#377; the project-rooted rail of Arugula #989; one
// button per place since Arugula #1345; the kit's since Lift 3, #1479): the
// same one as a machine's own page's (shared/kit/frame/rail), down the left
// on a computer, with your projects' marks under Arugula's. Then the places: Overview, Boards, Docs,
// Machines and Chat; at the foot the Inbox, Settings and you. A place's
// views are the bar's (`PlaceViews`), beside the path to where you are
// (./bar). A phone has no rail: the places are a bar along the bottom
// (`PhoneTabs`), and the Inbox a bell in the top bar (`PhoneInbox`).

import type { ComponentChildren } from "preact";
import { chat } from "../shared/chat-store";
import { chatUnread, projectUnread } from "../shared/chat/unread";
import { usePlatformApi } from "../shared/platform-api";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { BellIcon, BoardsIcon, ChatIcon, DocsIcon, MachinesIcon, Mark, OverviewIcon, SettingsIcon } from "../shared/ui/icons";
import { Rail, type ProjectMark, type RailPlace } from "../shared/kit/frame/rail";
import { Count } from "../shared/ui/state-chip";
import { AccountButton, chatPath, sectionOf } from "./bar";
import { chatTeam } from "./chat-scope";
import { currentProject, entryPath, onCurrentProject } from "./current-project";
import { useInboxCounts } from "./inbox";
import { leftAt } from "./goto-source";
import { owners } from "./owners";
import { placeOf } from "./places-kept";
import { names } from "./links";
import { follow, go, pathOf, useRoute, type Route } from "./router";
import { controlOrigin, session } from "./session";
import { openGoTo } from "../shared/kit/frame/goto";
import { surfacesFor } from "../shared/surfaces";
import { subscribeSeen } from "../shared/seen";
import { barProject, where } from "./where";

/** A place on the rail or the phone's bar, or a view in a place's bar. */
interface PlaceLink {
  key: string;
  label: string;
  tip: string;
  icon?: ComponentChildren;
  to: string;
  here: boolean;
  count?: { n: number; mention: boolean };
}

/** The rail's places (Arugula #1345). */
export type PlaceKey = "overview" | "boards" | "docs" | "machines" | "chat";

const none = { n: 0, mention: false };

/** The places and their views, for the project the page is in (else the
 * current one, else Personal's), as the rail, the bar and the phone's bar
 * draw them. */
function usePlaces() {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => chat.subscribe(fn));
  useSubscribe((fn) => where.subscribe(fn));
  useSubscribe(onCurrentProject);
  useSubscribe(subscribeSeen);
  const r = useRoute();
  const phone = usePhone();
  const { Machines } = usePlatformApi();
  // The only count across projects: the server's, as it is (Lift 6d, #1486).
  const inbox = useInboxCounts();
  const shown = barProject(r);
  const p = session.project(shown);
  // As the bar does: an id that's none of the reader's lights no place.
  const unknown = !!shown && !p && session.state === "ready";
  const section = unknown ? null : sectionOf(r);
  const project = p ?? session.project(currentProject()) ?? session.personal();
  const at = (page: "project" | "boards" | "docs" | "runs" | "workspaces") => (project ? pathOf({ page, project: project.id } as Route, names) : "/");
  const talk = !unknown ? chatUnread(chat.rooms, chatTeam(p, project)) : none;
  const chatTo = shown ? chatPath(shown) : entryPath("chat", project, names);
  // What's new in the threads on the project's issues (Boards) and runs (Machines, which holds Runs).
  const issues = !unknown && project && chat.on !== false ? projectUnread(chat.rooms, project.id, "iss_") : none;
  const runs = !unknown && project ? projectUnread(chat.rooms, project.id, "run_") : none;

  // The machines are the app's own places where it has them; else control's page.
  const m = r.page === "machines" ? r.at : null;
  const machines = !!Machines && !!project;
  const swarm = m?.at === "swarm";
  // Machines opens on the Machines page (Arugula #1346); a pane opens on its machine's page.
  const machinesTo = machines ? pathOf({ page: "machines", project: project.id, at: { at: "machines" } }, names) : `${controlOrigin()}/`;
  // The Swarm is the Overview's view (Arugula #1348 moves it in): its route lights the Overview.
  const overviewViews: PlaceLink[] = machines && !phone
    ? [
        { key: "overview", label: "Overview", tip: "Overview: this project's work", to: at("project"), here: section === "home" },
        { key: "swarm", label: "Swarm", tip: "Swarm: every pane on every machine", to: pathOf({ page: "machines", project: project.id, at: { at: "swarm" } }, names), here: swarm },
      ]
    : [];
  // A run is a session in a cloud workspace (decision 6): Runs and Workspaces are Machines' (Arugula #1082).
  const machineViews: PlaceLink[] = !machines
    ? []
    : [
        { key: "machines", label: "Machines", tip: "Machines: every machine's sessions and panes", to: machinesTo, here: !!m && !swarm },
        { key: "runs", label: "Runs", tip: "Runs: this project's runs", to: at("runs"), here: section === "runs", count: runs },
        ...(session.hosted ? [{ key: "workspaces", label: "Workspaces", tip: "Workspaces: this project's cloud workspaces", to: at("workspaces"), here: section === "workspaces" }] : []),
      ];
  const places: (PlaceLink & { key: PlaceKey; views: PlaceLink[] })[] = [
    // Deploys is hidden for v1 (Arugula #1078) but its page still opens from a link: it's the Overview's.
    { key: "overview", label: "Overview", tip: "Overview: this project's work", icon: <OverviewIcon />, to: at("project"), here: section === "home" || section === "deploys" || swarm, views: overviewViews },
    { key: "boards", label: "Boards", tip: "Boards: this project's boards and tickets", icon: <BoardsIcon />, to: at("boards"), here: section === "boards", count: issues, views: [] },
    { key: "docs", label: "Docs", tip: "Docs: this project's Docs", icon: <DocsIcon />, to: at("docs"), here: section === "docs", views: [] },
    { key: "machines", label: "Machines", tip: "Machines: your team's computers, their panes, cloud workspaces and runs", icon: <MachinesIcon />, to: machinesTo, here: (!!m && !swarm) || section === "workspaces" || section === "runs", views: machineViews },
    { key: "chat", label: "Chat", tip: "Chat: your team's rooms", icon: <ChatIcon />, to: chatTo, here: section === "chat", count: talk, views: [] },
  ];
  const foot: PlaceLink[] = [
    { key: "inbox", label: "Inbox", tip: "Inbox: what wants you, across your projects", icon: <BellIcon />, to: "/inbox", here: section === "inbox", count: { n: inbox.all, mention: false } },
    // Settings at the project you're in (Arugula #2594), else yours; account settings are also the avatar's menu.
    { key: "settings", label: "Settings", tip: shown ? "Settings: this project's" : "Settings", icon: <SettingsIcon />, to: shown && project ? pathOf({ page: "settings", scope: "project", project: project.id }, names) : "/settings", here: section === "settings" },
  ];
  return { places, foot, project, shown };
}

const external = (to: string) => /^https?:/.test(to);

const railPlace = (p: PlaceLink): RailPlace => ({
  key: p.key,
  label: p.label,
  tip: p.tip,
  icon: p.icon,
  here: p.here,
  count: p.count,
  href: p.to,
  // Control's page is a page load; the rest are this page's.
  onClick: external(p.to) ? undefined : () => go(p.to),
});

/** Your projects as the rail's marks (v6): each with what's for you there
 * and where you left it. */
function useMarks(shown: string | undefined): ProjectMark[] {
  const counts = useInboxCounts();
  return owners(session.projects).flatMap((o) =>
    o.projects.map((p) => {
      const kept = placeOf(p.id);
      return { id: p.id, name: p.name, here: p.id === shown, count: counts.by.get(p.id) ?? 0, left: leftAt(kept), go: () => go(kept ?? pathOf({ page: "project", project: p.id }, names)) };
    }),
  );
}

export function PlatformRail() {
  const { places, foot, shown } = usePlaces();
  const marks = useMarks(shown);
  const brand = (
    <a class="k-rail-brand" data-rail-brand href="/" title="Arugula: your projects" aria-label="Arugula" onClick={(e) => follow(e, "/")}>
      <Mark />
    </a>
  );
  const shows = surfacesFor(session.me?.account, shown).rail;
  return <Rail brand={brand} marks={marks} onProjects={() => openGoTo("projects")} places={places.filter((p) => shows.includes(p.key)).map(railPlace)} foot={foot.map(railPlace)} me={<AccountButton />} />;
}

/** The place you're in's views, in the bar (Arugula #1345): the Overview's
 * Swarm, Machines' Runs and Workspaces, as a machine's page has its own
 * (shell/ui/places' MachinePlaces). Boards, Docs and Chat have none. */
export function PlaceViews() {
  const { places } = usePlaces();
  const place = places.find((s) => s.here && s.views.length);
  if (!place) return null;
  return (
    <nav class="places" aria-label={`${place.label}'s views`} data-place-views={place.key}>
      {place.views.map((p) => (
        <a
          key={p.key}
          class={p.here ? "place frame-place here" : "place frame-place"}
          href={p.to}
          title={p.tip}
          aria-current={p.here ? "page" : undefined}
          {...{ [`data-view-${p.key}`]: "" }}
          onClick={(e) => !external(p.to) && follow(e, p.to)}
        >
          {p.label}
          {!!p.count?.n && <Count for={p.count.mention ? "mention" : "you"} class="count-after">{p.count.mention ? `@${p.count.n}` : p.count.n}</Count>}
        </a>
      ))}
    </nav>
  );
}

/** The phone's bar along the bottom (decision 15; Arugula #1345): the
 * rail's five places. A place's views are the top bar's; the Inbox is its
 * bell (`PhoneInbox`) and Settings is under your picture. */
export function PhoneTabs() {
  const { places } = usePlaces();
  return (
    <nav class="app-tabs" aria-label="Places" data-phone-tabs>
      {places.map((p) => (
        <a key={p.key} class={p.here ? "app-tab here" : "app-tab"} href={p.to} aria-current={p.here ? "page" : undefined} {...{ [`data-tab-${p.key}`]: "" }} onClick={(e) => !external(p.to) && (e.preventDefault(), go(p.to))}>
          {p.icon}
          <span>{p.label}</span>
          {!!p.count?.n && <span class={p.count.mention ? "k-rail-count is-mention" : "k-rail-count"} data-count>{p.count.mention ? `@${p.count.n}` : p.count.n}</span>}
        </a>
      ))}
    </nav>
  );
}

/** The Inbox on a phone (Arugula #1345): a bell in the top bar, with its count. */
export function PhoneInbox() {
  const { foot } = usePlaces();
  const inbox = foot.find((p) => p.key === "inbox")!;
  const n = inbox.count?.n ?? 0;
  return (
    <a
      class={inbox.here ? "platform-inbox-bell here" : "platform-inbox-bell"}
      href={inbox.to}
      title={inbox.tip}
      aria-label={n ? `Inbox, ${n}${inbox.count?.mention ? ", for you" : ""}` : "Inbox"}
      aria-current={inbox.here ? "page" : undefined}
      data-phone-inbox
      onClick={(e) => follow(e, inbox.to)}
    >
      <BellIcon />
      {!!n && <span class={inbox.count?.mention ? "k-rail-count is-mention" : "k-rail-count"} data-count>{inbox.count?.mention ? `@${n}` : n}</span>}
    </a>
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
