// The rail down the left of a machine's own page (M108, #1023): control's
// page and a daemon's. The app's three spaces (Arugula #1081, as the app's
// rail: platform/rail.tsx): Plan and Talk are the app's (the platform page's
// Overview and Chat, M153), and Build is this page, its places the bar's
// (MachinePlaces: Panes, Swarm, Machines). Arugula's mark (this machine's
// sessions) is above them, Settings at the foot. Plan and Talk are only
// where control has a platform page.
// It stays while the swarm or agents cover the panes, so the way back is
// always in one place, and Build goes back to the session's last tab and
// pane (the routes never change the selection). The rail itself is
// shared/ui/rail.tsx; this builds this page's items.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../../client";
import { directory } from "../../hosts";
import { onSwarmRoute } from "../../swarm/route";
import { openSettings } from "../settings-route";
import { Rail as SharedRail, type RailItem } from "../../../shared/ui/rail";
import { openPlatform, platformHref } from "../../platform-links";
import { PROJECTS_TO, chatUnread, closePlace, currentPlace, hasChat, openChat, openOverview, useChatTick, type Place } from "../places";
import { countView } from "../../../shared/telemetry";
import { BuildIcon, Mark, PlanIcon, SettingsIcon, TalkIcon } from "../../../shared/ui/icons";
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
  // The current project's Overview, which the platform page knows (Arugula
  // #880), or the list on a first visit.
  const plan = platformHref(PROJECTS_TO);
  const build = () => {
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
    !!plan && {
      label: "Plan",
      tip: "Plan: your project's overview, boards and Docs",
      here: false,
      hook: "data-open-plan",
      icon: <PlanIcon />,
      href: plan,
      target: "arugula-platform",
      onClick: () => openPlatform(PROJECTS_TO),
    },
    // This page: the panes, the swarm, this machine's Docs and recipes.
    { label: "Build", tip: "Build: your panes, the swarm and your team's machines", here: at !== "settings", hook: "data-open-build", icon: <BuildIcon />, onClick: build },
    threads && {
      label: "Talk",
      aria: n ? `Talk, ${n} unread${mention ? ", mentioning you" : ""}` : "Talk",
      tip: "Talk: your team's rooms, and every thread on every machine",
      here: false,
      hook: "data-open-talk",
      icon: <TalkIcon />,
      count: { n, mention },
      onClick: () => openChat(),
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
