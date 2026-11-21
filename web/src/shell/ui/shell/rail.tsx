// The rail down the left of a machine's own page (M108, #1023): control's
// page and a daemon's. The app's places (Arugula #1345, as the app's rail:
// platform/rail.tsx): Overview, Boards, Docs and Chat are the app's (the
// platform page's, M153), and Machines is this page, its views the bar's
// (MachinePlaces). Arugula's mark (this machine's sessions) is above them,
// the Inbox and Settings at the foot. The app's places are only where
// control has a platform page.
// It stays while the swarm or agents cover the panes, so the way back is
// always in one place, and Machines goes back to the session's last tab and
// pane (the routes never change the selection). The rail itself is
// shared/ui/rail.tsx; this builds this page's items.

import type { ComponentChildren } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { Client } from "../../client";
import { directory } from "../../hosts";
import { onSwarmRoute } from "../../swarm/route";
import { openSettings } from "../settings-route";
import { Rail as SharedRail, type RailItem } from "../../../shared/ui/rail";
import { openPlatform, platformHref } from "../../platform-links";
import { BOARDS_TO, DOCS_TO, INBOX_TO, PROJECTS_TO, chatUnread, closePlace, currentPlace, hasChat, openChat, openOverview, useChatTick, type Place } from "../places";
import { countView } from "../../../shared/telemetry";
import { BellIcon, BoardsIcon, ChatIcon, DocsIcon, MachinesIcon, Mark, OverviewIcon, SettingsIcon } from "../../../shared/ui/icons";
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
  const settings = () => {
    countView("settings");
    openSettings();
  };
  const machines = () => {
    if (at !== "panes") closePlace(at);
    else client.viewOf(client.active() ?? -1)?.focus();
  };
  /** One of the app's places, where control has a platform page (the
   * current project's, which it knows, Arugula #880): its own tab, or this
   * window in the app (openPlatform); a modified click is the browser's. */
  const app = (to: string, label: string, tip: string, hook: string, icon: ComponentChildren): RailItem | false => {
    const href = platformHref(to);
    return !!href && { label, tip, here: false, hook, icon, href, target: "arugula-platform", onClick: () => openPlatform(to) };
  };
  const items: (RailItem | false)[] = [
    app(PROJECTS_TO, "Overview", "Overview: your project's work", "data-open-overview", <OverviewIcon />),
    app(BOARDS_TO, "Boards", "Boards: your project's boards and tickets", "data-open-boards", <BoardsIcon />),
    app(DOCS_TO, "Docs", "Docs: your project's Docs", "data-open-docs", <DocsIcon />),
    // This page: the panes, the swarm, this machine's Docs and recipes.
    { label: "Machines", tip: "Machines: your panes, the swarm and your team's machines", here: at !== "settings", hook: "data-open-machines", icon: <MachinesIcon />, onClick: machines },
    threads && {
      label: "Chat",
      aria: n ? `Chat, ${n} unread${mention ? ", mentioning you" : ""}` : "Chat",
      tip: "Chat: your team's rooms, and every thread on every machine",
      here: false,
      hook: "data-open-chat",
      icon: <ChatIcon />,
      count: { n, mention },
      onClick: () => openChat(),
    },
  ];
  const brand = (
    <button
      class="ws-brand"
      aria-label={`Arugula: the sessions on ${host}`}
      title={`The sessions on ${host}: their tabs, panes and what needs you`}
      data-open-sessions-overview
      onClick={() => (at === "overview" ? focusSessionSearch() : openOverview())}
    >
      <Mark />
    </button>
  );
  const bottom: (RailItem | false)[] = [
    app(INBOX_TO, "Inbox", "Inbox: what wants you, across your projects", "data-open-inbox", <BellIcon />),
    { label: "Settings", tip: "Settings: this device, this machine and your account", here: at === "settings", hook: "data-open-settings", icon: <SettingsIcon />, onClick: settings },
  ];
  return <SharedRail brand={brand} items={items.filter((i): i is RailItem => !!i)} bottom={bottom.filter((i): i is RailItem => !!i)} />;
}
