// The platform page's rail (#377): the same one as the panes page's
// (shared/ui/rail), down the left of the page on a computer. Projects, Chat and Docs
// are places in this page; Panes and Swarm are links to control's page, in
// this window (no `target`: the desktop app keeps the
// platform page and control's in one window each, #288). The bar beside it
// (./bar) keeps the project and its places.

import type { ComponentChildren } from "preact";
import { chat } from "../shared/chat-store";
import { chatUnread } from "../shared/chat/unread";
import { usePhone, useSubscribe } from "../shared/ui/hooks";
import { Rail, type RailItem } from "../shared/ui/rail";
import { ChatIcon, DocsIcon, Mark, PanesIcon, ProjectsIcon, SettingsIcon, SwarmIcon } from "../shared/ui/icons";
import { chatPath, sectionOf } from "./bar";
import { currentProject, entryPath, onCurrentProject } from "./current-project";
import { go, useRoute } from "./router";
import { names } from "./links";
import { controlOrigin, session } from "./session";
import { barProject, where } from "./where";

export function PlatformRail() {
  useSubscribe((fn) => session.subscribe(fn));
  useSubscribe((fn) => chat.subscribe(fn));
  useSubscribe((fn) => where.subscribe(fn));
  useSubscribe(onCurrentProject);
  const r = useRoute();
  const project = barProject(r);
  const p = session.project(project);
  // As the bar does: an id that's none of the reader's lights no place.
  const unknown = !!project && !p && session.state === "ready";
  const here = unknown ? null : sectionOf(r);
  const control = controlOrigin();
  // Where a button opens: the project the bar is in, else the current one (Arugula #880).
  const inProject = session.project(project ?? currentProject());
  const chatTo = project ? chatPath(project) : entryPath("chat", inProject, names);
  const { n, mention } = !unknown && (!project || p) ? chatUnread(chat.rooms, p ? (p.team?.id ?? null) : undefined) : { n: 0, mention: false };
  const place = (to: string) => () => go(to);
  // Projects goes to the current project while it's still one of the reader's; else the list.
  const projectsTo = entryPath("projects", inProject, names);
  const docsTo = entryPath("docs", inProject, names);
  const items: RailItem[] = [
    {
      label: "Projects",
      tip: "Projects: their issues, boards, runs, deploys and chat",
      // In any project and across All projects' places (the bar's), but Chat
      // and Docs are lit on their own.
      here: !unknown && (r.page === "home" || !!project || here === "boards" || here === "inbox") && here !== "chat" && here !== "docs",
      hook: "data-open-projects",
      icon: <ProjectsIcon />,
      href: projectsTo,
      onClick: place(projectsTo),
    },
    { label: "Panes", tip: "Your panes, on your machines", here: false, hook: "data-open-panes", icon: <PanesIcon />, href: `${control}/` },
    { label: "Swarm", tip: "Swarm: every agent on every machine", here: false, hook: "data-open-swarm", icon: <SwarmIcon />, href: `${control}/#swarm` },
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
    // A project's Docs, or with none current the team Docs and the reader's own;
    // lit on every Docs page: a project's and a Doc's too.
    {
      label: "Docs",
      tip: "Docs: your team's Docs, and yours from your machines",
      here: !unknown && here === "docs",
      hook: "data-open-docs-all",
      icon: <DocsIcon />,
      href: docsTo,
      onClick: place(docsTo),
    },
  ];
  const brand = (
    <a class="ws-brand" href={`${control}/`} aria-label="Arugula: your machines and sessions" title="Your machines and sessions" data-open-control>
      <Mark />
    </a>
  );
  const bottom: RailItem[] = [
    {
      label: "Settings",
      tip: "Settings: your plan, agents and chat here; this device and machine on your machines page",
      // A project's settings are the bar's.
      here: r.page === "settings",
      hook: "data-open-settings",
      icon: <SettingsIcon />,
      href: "/settings",
      onClick: place("/settings"),
    },
  ];
  return <Rail brand={brand} items={items} bottom={bottom} />;
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
