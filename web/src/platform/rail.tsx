// The platform page's rail (#377): the same one as the panes page's
// (shared/ui/rail), down the left of the page on a computer. Projects, Chat
// and Boards are places in this page; Panes, Swarm and Docs are links to
// control's page, in this window (no `target`: the desktop app keeps the
// platform page and control's in one window each, #288). The bar beside it
// (./bar) keeps the project and its places.

import type { ComponentChildren } from "preact";
import { chat } from "../shared/chat-store";
import { chatUnread } from "../shared/chat/unread";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { Rail, type RailItem } from "../shared/ui/rail";
import { BoardsIcon, ChatIcon, DocsIcon, Mark, PanesIcon, ProjectsIcon, SettingsIcon, SwarmIcon } from "../shared/ui/rail-icons";
import { chatPath, sectionOf } from "./bar";
import { useBoardsOn } from "./pages/boards";
import { go, useRoute } from "./router";
import { controlOrigin, session } from "./session";
import { barProject, where } from "./where";

export function PlatformRail() {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => chat.subscribe(fn));
  useSubscribe((fn) => where.subscribe(fn));
  const r = useRoute();
  const project = barProject(r);
  const p = session.project(project);
  // As the bar does: an id that's none of the reader's lights no place.
  const unknown = !!project && !p && session.state === "ready";
  const here = unknown ? null : sectionOf(r);
  const boards = useBoardsOn();
  const control = controlOrigin();
  const chatTo = chatPath(project);
  const { n, mention } = !unknown && (!project || p) ? chatUnread(chat.rooms, p ? (p.team?.id ?? null) : undefined) : { n: 0, mention: false };
  const place = (to: string) => () => go(to);
  const items: (RailItem | false)[] = [
    {
      label: "Projects",
      tip: "Projects: their issues, boards, runs, deploys and chat",
      // In any project, but Chat and Boards are lit on theirs.
      here: !unknown && (r.page === "home" || !!project) && here !== "chat" && here !== "boards",
      hook: "data-open-projects",
      icon: <ProjectsIcon />,
      href: "/",
      onClick: place("/"),
    },
    { label: "Panes", tip: "Your panes, on Control", here: false, hook: "data-open-panes", icon: <PanesIcon />, href: `${control}/` },
    { label: "Swarm", tip: "Swarm: every agent on every machine, on Control", here: false, hook: "data-open-swarm", icon: <SwarmIcon />, href: `${control}/#swarm` },
    {
      label: "Chat",
      aria: n ? `Chat, ${n} unread${mention ? ", mentioning you" : ""}` : "Chat",
      tip: "Chat: every thread, on every machine",
      here: here === "chat",
      hook: "data-open-chat",
      icon: <ChatIcon />,
      count: { n, mention },
      href: chatTo,
      onClick: place(chatTo),
    },
    // With the `boards` flag, which is a team's: on in any of the reader's projects.
    (boards || here === "boards") && {
      label: "Boards",
      tip: "Boards: your projects' boards and epics",
      here: here === "boards",
      hook: "data-open-boards",
      icon: <BoardsIcon />,
      href: "/boards",
      onClick: place("/boards"),
    },
    { label: "Docs", tip: "Docs: the Docs your agents wrote, to open again", here: false, hook: "data-open-docs", icon: <DocsIcon />, href: `${control}/#docs` },
  ];
  const brand = (
    <a class="ws-brand" href={`${control}/`} aria-label="Arugula: your machines and sessions" title="Your machines and sessions, on Control" data-open-control>
      <Mark />
    </a>
  );
  const bottom: RailItem[] = [
    {
      label: "Settings",
      tip: "Account settings",
      // A project's settings are the bar's.
      here: r.page === "settings",
      hook: "data-open-settings",
      icon: <SettingsIcon />,
      href: "/settings",
      onClick: place("/settings"),
    },
  ];
  return <Rail brand={brand} items={items.filter((i): i is RailItem => !!i)} bottom={bottom} />;
}

/** The page's body beside its rail: the rail on a computer, none on a phone. */
export function WithRail({ children }: { children: ComponentChildren }) {
  const phone = usePhone();
  return (
    <>
      {!phone && <PlatformRail />}
      <div class="ws-shell-body platform-body">{children}</div>
    </>
  );
}
