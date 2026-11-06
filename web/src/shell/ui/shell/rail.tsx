// The rail down the left of the workspace (M108, #1023): the whole app, with
// the same places as the platform page's (Arugula #640). Arugula's mark (this
// machine's sessions), Panes, Swarm, Chat (on the platform page, M153) and
// Settings, with Projects (the platform page's `/`) above them all (#376). A
// machine's Docs and Recipes are its bar's (MachinePlaces).
// It stays while the swarm or agents cover the panes, so the way back is
// always in one place, and Panes goes back to the session's last tab and
// pane (the routes never change the selection). The rail itself is
// shared/ui/rail.tsx; this builds the panes page's items.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../../client";
import { directory } from "../../hosts";
import { SWARM_ABOUT, onSwarmRoute, openSwarm } from "../../swarm/route";
import { openSettings } from "../settings-route";
import { Rail as SharedRail, type RailItem } from "../../../shared/ui/rail";
import { openPlatform, platformHref } from "../../platform-links";
import { chatUnread, closePlace, currentPlace, hasChat, openChat, openOverview, useChatTick, type Place } from "../places";
import { countView } from "../../../shared/telemetry";
import { ChatIcon, DocsIcon, Mark, PanesIcon, ProjectsIcon, SettingsIcon, SwarmIcon } from "./icons";
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
  const host = directory.current || "this machine";
  // Back to the page you left there (Arugula #638), or the list on a first visit.
  const PROJECTS_PATH = "/?back";
  const projects = platformHref(PROJECTS_PATH);
  const DOCS_PATH = "/docs";
  const teamDocs = platformHref(DOCS_PATH);
  const panes = () => {
    if (at !== "panes") closePlace(at);
    else client.viewOf(client.active() ?? -1)?.focus();
  };
  const settings = () => {
    countView("settings");
    openSettings();
  };
  const items: (RailItem | false)[] = [
    // Only where control has a platform page. Its own tab, or this window in
    // the app (openPlatform); a modified click is the browser's.
    !!projects && {
      label: "Projects",
      tip: "Projects: their issues, boards, runs, deploys and chat",
      here: false,
      hook: "data-open-projects",
      icon: <ProjectsIcon />,
      href: projects,
      target: "arugula-platform",
      onClick: () => openPlatform(PROJECTS_PATH),
    },
    { label: "Panes", tip: "Your panes: back to this session's last tab and pane", here: at === "panes" || at === "docs" || at === "agents", hook: "data-open-panes", icon: <PanesIcon />, onClick: panes },
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
      tip: "Chat: every thread, on every machine",
      here: false,
      hook: "data-open-chat",
      icon: <ChatIcon />,
      count: { n, mention },
      onClick: () => openChat(),
    },
    // The platform's /docs: the team's Docs and the reader's from every machine.
    // This machine's own are its bar's.
    !!teamDocs && {
      label: "Docs",
      tip: "Docs: your team's Docs, and yours from your machines",
      here: false,
      hook: "data-open-docs-all",
      icon: <DocsIcon />,
      href: teamDocs,
      target: "arugula-platform",
      onClick: () => openPlatform(DOCS_PATH),
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
    { label: "Settings", tip: "Settings: this device, this machine and your account", here: at === "settings", hook: "data-open-settings", icon: <SettingsIcon />, onClick: settings },
  ];
  return <SharedRail brand={brand} items={items.filter((i): i is RailItem => !!i)} bottom={bottom} />;
}
