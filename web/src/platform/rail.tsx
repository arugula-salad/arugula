// The app's rail (#377; the project-rooted rail of Arugula #989; three
// spaces since Arugula #1080): the same one as a machine's own page's
// (shared/ui/rail), down the left on a computer. The project tile at the
// top is the switcher. Then the three kinds of work: Plan (Overview,
// Boards, Docs), Build (Panes, Swarm, Machines) and Talk (Chat); at the
// foot the Inbox, Settings and you. A space's places are the bar's
// (`SpacePlaces`), beside the path to where you are (./bar). A phone has no
// rail: the spaces and the Inbox are a bar along the bottom (`PhoneTabs`).

import type { ComponentChildren } from "preact";
import { useRef, useState } from "preact/hooks";
import { chat } from "../shared/chat-store";
import { chatUnread, projectUnread } from "../shared/chat/unread";
import { usePlatformApi } from "../shared/platform-api";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { BellIcon, BuildIcon, PlanIcon, SettingsIcon, TalkIcon } from "../shared/ui/icons";
import { pressedShut } from "../shared/ui/menu";
import { Rail, type RailItem } from "../shared/ui/rail";
import { Count } from "../shared/ui/state-chip";
import { AccountButton, chatPath, sectionOf } from "./bar";
import { chatTeam } from "./chat-scope";
import { currentProject, entryPath, onCurrentProject } from "./current-project";
import { useInboxCount } from "./inbox-count";
import { names } from "./links";
import { ownerKey } from "./owners";
import { follow, go, pathOf, useRoute, type Route } from "./router";
import { controlOrigin, session } from "./session";
import { Switcher } from "./switcher";
import { subscribeSeen } from "../shared/seen";
import { barProject, where } from "./where";

/** A space on the rail or the phone's bar, or a place in a space's bar. */
interface PlaceLink {
  key: string;
  label: string;
  tip: string;
  icon?: ComponentChildren;
  to: string;
  here: boolean;
  count?: { n: number; mention: boolean };
}

/** The three kinds of work (Arugula #1080). */
export type Space = "plan" | "build" | "talk";

const none = { n: 0, mention: false };

/** The spaces and their places, for the project the page is in (else the
 * current one, else Personal's), as the rail, the bar and the phone's bar
 * draw them. */
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
  const at = (page: "project" | "boards" | "docs") => (project ? pathOf({ page, project: project.id } as Route, names) : "/");
  const talk = !unknown ? chatUnread(chat.rooms, chatTeam(p, project)) : none;
  const chatTo = shown ? chatPath(shown) : entryPath("chat", project, names);
  // What's new in the threads on the project's issues (Boards) and runs (Machines, which holds Runs).
  const issues = !unknown && project && chat.on !== false ? projectUnread(chat.rooms, project.id, "iss_") : none;
  const runs = !unknown && project ? projectUnread(chat.rooms, project.id, "run_") : none;

  const plan: PlaceLink[] = [
    { key: "overview", label: "Overview", tip: "Overview: this project's work", to: at("project"), here: section === "home" },
    { key: "boards", label: "Boards", tip: "Boards: this project's boards and tickets", to: at("boards"), here: section === "boards" && !!shown, count: issues },
    { key: "docs", label: "Docs", tip: "Docs: this project's Docs", to: at("docs"), here: section === "docs" },
  ];
  // The machines are the app's own places where it has them; else control's page.
  const m = r.page === "machines" ? r.at : null;
  const list = !!m && m.at === "machines" && m.pane === undefined && !m.name;
  const machinesList = project ? pathOf({ page: "machines", project: project.id, at: { at: "machines" } }, names) : "/";
  // Build opens on every machine's panes, by machine (Arugula #1083); a pane opens on its machine's page, still Panes'.
  const panesTo = Machines && project ? pathOf({ page: "machines", project: project.id, at: { at: "panes" } }, names) : `${controlOrigin()}/`;
  const build: PlaceLink[] = !Machines || !project
    ? []
    : [
        { key: "panes", label: "Panes", tip: "Panes: every pane on every machine, by machine", to: panesTo, here: !!m && !list && m.at !== "swarm" },
        { key: "swarm", label: "Swarm", tip: "Swarm: every pane on every machine", to: pathOf({ page: "machines", project: project.id, at: { at: "swarm" } }, names), here: m?.at === "swarm" },
        // A run is a session in a cloud workspace (decision 6): Runs and Workspaces are under Machines (Arugula #1082).
        { key: "machines", label: "Machines", tip: "Machines: your team's computers, cloud workspaces and runs", to: machinesList, here: list || section === "workspaces" || section === "runs", count: runs },
      ];
  // Deploys is hidden for v1 (Arugula #1078) but its page still opens from a link: it's planning's.
  const inPlan = !!section && ["home", "boards", "docs", "deploys"].includes(section);
  const spaces: (PlaceLink & { key: Space; places: PlaceLink[] })[] = [
    { key: "plan", label: "Plan", tip: "Plan: this project's overview, boards and Docs", icon: <PlanIcon />, to: at("project"), here: inPlan, count: issues, places: plan },
    { key: "build", label: "Build", tip: "Build: your panes, the swarm and your team's machines", icon: <BuildIcon />, to: panesTo, here: r.page === "machines" || section === "workspaces" || section === "runs", count: runs, places: build },
    { key: "talk", label: "Talk", tip: "Talk: your team's rooms", icon: <TalkIcon />, to: chatTo, here: section === "chat", count: talk, places: [] },
  ];
  const foot: PlaceLink[] = [
    { key: "inbox", label: "Inbox", tip: inbox.title || "Inbox: what wants you, across your projects", icon: <BellIcon />, to: "/inbox", here: section === "inbox", count: { n: inbox.n, mention: inbox.waiting > 0 } },
    // Settings at the project you're in (Arugula #2594), else yours; account settings are also the avatar's menu.
    { key: "settings", label: "Settings", tip: shown ? "Settings: this project's" : "Settings", icon: <SettingsIcon />, to: shown && project ? pathOf({ page: "settings", scope: "project", project: project.id }, names) : "/settings", here: section === "settings" },
  ];
  return { spaces, foot, project, shown };
}

const external = (to: string) => /^https?:/.test(to);

const railItem = (p: PlaceLink): RailItem => ({
  label: p.label,
  tip: p.tip,
  aria: p.count?.n ? `${p.label}, ${p.count.n}${p.count.mention ? ", for you" : ""}` : undefined,
  here: p.here,
  hook: `data-open-${p.key}`,
  icon: p.icon,
  count: p.count,
  href: p.to,
  // Control's page is a page load; the rest are this page's.
  onClick: external(p.to) ? undefined : () => go(p.to),
});

export function PlatformRail() {
  const { spaces, foot, project, shown } = usePlaces();
  const brand = <ProjectTile project={project} shown={shown} />;
  return <Rail brand={brand} items={spaces.map(railItem)} bottom={foot.map(railItem)} foot={<AccountButton compact />} />;
}

/** The space you're in's places, in the bar (Arugula #1080), as a
 * machine's page has its own (shell/ui/places' MachinePlaces). Talk has
 * none: Chat is the whole of it. */
export function SpacePlaces() {
  const { spaces } = usePlaces();
  const space = spaces.find((s) => s.here && s.places.length);
  if (!space) return null;
  return (
    <nav class="places" aria-label={`${space.label}'s places`} data-space-places={space.key}>
      {space.places.map((p) => (
        <a
          key={p.key}
          class={p.here ? "place frame-place here" : "place frame-place"}
          href={p.to}
          title={p.tip}
          aria-current={p.here ? "page" : undefined}
          {...{ [`data-space-${p.key}`]: "" }}
          onClick={(e) => !external(p.to) && follow(e, p.to)}
        >
          {p.label}
          {!!p.count?.n && <Count for={p.count.mention ? "mention" : "you"} class="count-after">{p.count.mention ? `@${p.count.n}` : p.count.n}</Count>}
        </a>
      ))}
    </nav>
  );
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

/** The phone's bar along the bottom (decision 15; Arugula #1080): Plan,
 * Build, Talk and the Inbox. A space's places are the top bar's; Settings
 * is under your picture. */
export function PhoneTabs() {
  const { spaces, foot } = usePlaces();
  const tabs = [...spaces, foot.find((p) => p.key === "inbox")!];
  return (
    <nav class="app-tabs" aria-label="Places" data-phone-tabs>
      {tabs.map((p) => (
        <a key={p.key} class={p.here ? "app-tab here" : "app-tab"} href={p.to} aria-current={p.here ? "page" : undefined} {...{ [`data-tab-${p.key}`]: "" }} onClick={(e) => !external(p.to) && (e.preventDefault(), go(p.to))}>
          {p.icon}
          <span>{p.label}</span>
          {!!p.count?.n && <span class={p.count.mention ? "ws-rail-count mention" : "ws-rail-count"}>{p.count.mention ? `@${p.count.n}` : p.count.n}</span>}
        </a>
      ))}
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
