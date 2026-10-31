// The rail down the left of the workspace (M108, #1023): Arugula's mark
// (this machine's sessions), Panes, Swarm, Chat (on
// the platform page, M153), Boards (with the `boards` flag), Docs, Agents and
// Settings, with Projects (the platform page's `/`) above them all (#376).
// It stays while the swarm or agents cover the panes, so the way back is
// always in one place, and Panes goes back to the session's last tab and
// pane (the routes never change the selection). The rail itself is
// shared/ui/rail.tsx; this builds the panes page's items.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../../client";
import { directory } from "../../hosts";
import { SWARM_ABOUT, onSwarmRoute, openSwarm } from "../../swarm/route";
import { openAgentsPage } from "../agents-route";
import { openDocsPage } from "../docs-route";
import { settingsItems } from "../commands";
import { openMenu } from "../../../shared/ui/menu";
import { Rail as SharedRail, type RailItem } from "../../../shared/ui/rail";
import { openPlatform, platformHref } from "../../platform-links";
import { chatUnread, closePlace, currentPlace, hasBoards, hasChat, openBoards, openChat, openOverview, useChatTick, type Place } from "../places";
import { countView } from "../../../shared/telemetry";
import { AgentsIcon, BoardsIcon, ChatIcon, DocsIcon, Mark, PanesIcon, ProjectsIcon, SettingsIcon, SwarmIcon } from "./icons";
import { focusSessionSearch } from "./prefs";

/** Re-render on any route change, the swarm's too (it closes without a
 * hashchange). */
export function usePlace(client: Client): Place {
  useChatTick(client);
  const [, tick] = useState(0);
  useEffect(() => {
    const fn = () => tick((t) => t + 1);
    const off = onSwarmRoute(fn);
    addEventListener("hashchange", fn);
    return () => {
      off();
      removeEventListener("hashchange", fn);
    };
  }, []);
  return currentPlace();
}

export function Rail({ client }: { client: Client }) {
  const at = usePlace(client);
  const threads = hasChat();
  const { n, mention } = threads ? chatUnread() : { n: 0, mention: false };
  // As places.tsx decides: the machine owner's, Agents with the Labs flag.
  const agents = client.flag("agent-delegation") && !client.state?.roles;
  const docs = !client.state?.roles;
  const host = directory.current || "this machine";
  const projects = platformHref("/");
  const panes = () => {
    if (at !== "panes") closePlace(at);
    else client.viewOf(client.active() ?? -1)?.focus();
  };
  const settings = (e: MouseEvent) => {
    countView("settings");
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openMenu({ clientX: r.right + 4, clientY: r.top, preventDefault: () => e.preventDefault() }, settingsItems(client, { machine: true }), e.currentTarget as HTMLElement);
  };
  const items: (RailItem | false)[] = [
    // Only where control has a platform page. Its own tab, or this window in
    // the app (openPlatform); a modified click is the browser's.
    !!projects && {
      label: "Projects",
      tip: "Projects: their issues, boards, runs, deploys and chat, on the platform page",
      here: false,
      hook: "data-open-projects",
      icon: <ProjectsIcon />,
      href: projects,
      target: "arugula-platform",
      onClick: () => openPlatform("/"),
    },
    { label: "Panes", tip: "Your panes: back to this session's last tab and pane", here: at === "panes", hook: "data-open-panes", icon: <PanesIcon />, onClick: panes },
    {
      label: "Swarm",
      aria: `Swarm: ${SWARM_ABOUT}`,
      tip: `Swarm: ${SWARM_ABOUT}. Panes brings you back here.`,
      here: at === "swarm",
      hook: "data-open-swarm",
      icon: <SwarmIcon />,
      onClick: () => at !== "swarm" && openSwarm(),
    },
    threads && {
      label: "Chat",
      aria: n ? `Chat, ${n} unread${mention ? ", mentioning you" : ""}` : "Chat",
      tip: "Chat: every thread, on every machine, on the platform page",
      here: false,
      hook: "data-open-chat",
      icon: <ChatIcon />,
      count: { n, mention },
      onClick: () => openChat(),
    },
    hasBoards(client) && {
      label: "Boards",
      tip: "Boards: your projects' boards and epics, on the platform page",
      here: false,
      hook: "data-open-boards",
      icon: <BoardsIcon />,
      onClick: () => openBoards(),
    },
    docs && {
      label: "Docs",
      tip: "Docs: the Docs your agents wrote, to open again",
      here: at === "docs",
      hook: "data-open-docs",
      icon: <DocsIcon />,
      onClick: () => at !== "docs" && openDocsPage(),
    },
    agents && {
      label: "Agents",
      tip: "Agents: this machine's recipes and your team's agents",
      here: at === "agents",
      hook: "data-open-agents",
      icon: <AgentsIcon />,
      onClick: () => at !== "agents" && openAgentsPage(),
    },
  ];
  const brand = (
    <button
      class="ws-brand"
      aria-label={`Arugula: the sessions on ${host}`}
      title={`The sessions on ${host}: their tabs, panes and what needs you`}
      data-open-overview
      onClick={() => (at === "overview" ? focusSessionSearch() : openOverview())}
    >
      <Mark />
    </button>
  );
  const bottom: RailItem[] = [
    { label: "Settings", tip: "Appearance, notifications and this machine's settings", here: false, hook: "data-open-settings", icon: <SettingsIcon />, onClick: settings },
  ];
  return <SharedRail brand={brand} items={items.filter((i): i is RailItem => !!i)} bottom={bottom} />;
}
