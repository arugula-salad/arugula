// The Starred group's rows (#854), for the Sessions sidebar, the phone's
// Here sheet and the palette: what each starred tab or pane is called,
// where it is now, and a jump to it.

import { tabLabel, type Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { starredList, type Starred } from "../stars";
import { closePlace, currentPlace } from "./places";
import { paneName } from "./shell/overview";

export interface StarredRow {
  /** `tab:N` or `pane:N`. */
  key: string;
  star: Starred;
  /** The tab's label, or the pane's name. */
  label: string;
  /** The palette's detail line: session, cwd and branch. */
  detail: string;
}

/** A directory with the home folder as `~`. */
export const homeCwd = (cwd: string | null) => cwd?.replace(/^\/(?:home|Users)\/[^/]+/, "~") ?? null;

/** Where a pane is now (its session and tab, read from the layout, as a
 * moved pane's star follows it), its directory and its branch. */
export function paneDetail(client: Client, pane: PaneId): string {
  const tab = client.tabOfPane(pane);
  const session = client.state?.sessions.find((s) => s.id === client.sessionOfPane(pane));
  const where = [session?.name, tab && tabLabel(client, tab)].filter(Boolean).join(" › ");
  return [where, homeCwd(client.cwd(pane)), client.info(pane)?.branch].filter(Boolean).join(" · ");
}

/** The starred tabs and panes this client sees, oldest star first. */
export function starredRows(client: Client): StarredRow[] {
  return starredList(client.state).flatMap((star): StarredRow[] => {
    if (star.kind === "tab") {
      const tab = client.tabView(star.id);
      if (!tab) return [];
      const session = client.state?.sessions.find((s) => s.id === client.sessionOfTab(star.id));
      const pane = client.active(star.id);
      const detail = [session?.name, pane === undefined ? null : homeCwd(client.cwd(pane)), pane === undefined ? null : client.info(pane)?.branch];
      return [{ key: `tab:${star.id}`, star, label: tabLabel(client, tab), detail: detail.filter(Boolean).join(" · ") }];
    }
    const info = client.info(star.id);
    if (!info) return [];
    return [{ key: `pane:${star.id}`, star, label: paneName(client, info), detail: paneDetail(client, star.id) }];
  });
}

/** Whether a row is what's shown now. */
export function isCurrent(client: Client, star: Starred): boolean {
  return star.kind === "tab" ? client.tab === star.id : client.tab === client.tabOfPane(star.id)?.id && client.active() === star.id;
}

/** Show a starred tab or pane, closing any page over the panes (the
 * overview, the swarm, Docs, Agents), as showing a session does. */
export function goToStarred(client: Client, star: Starred) {
  const at = currentPlace();
  if (at !== "panes") closePlace(at);
  if (star.kind === "tab") client.selectTab(star.id);
  else client.setActive(star.id);
  const pane = star.kind === "tab" ? client.active(star.id) : star.id;
  if (pane !== undefined) requestAnimationFrame(() => client.viewOf(pane)?.focus());
}
