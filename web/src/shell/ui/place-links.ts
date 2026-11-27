// Where the shell's buttons go (M153; Lift 6a, #1483): chat, Boards, Docs
// and the Inbox are on the platform page, and Machines is the app's page.
// The shell draws no chat page of its own since M155, and no places bar
// since Lift 6a: the rail (./shell/rail) and Go to… have the places.

import { closeSettings, settingsRoute } from "./settings-route";
import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { directory } from "../hosts";
import { DOCS_TO } from "./docs-route";
import { getFleet } from "./hosts";
import { closeThread } from "./threads";
import { chat, chatStore } from "../../shared/chat-store";
import { landOnPlatform, openPlatform, platformHref, platformOrigin } from "../platform-links";
import { machinesPath } from "../../shared/machine-paths";

// ---- chat, on the platform page

const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
addEventListener("hashchange", changed);

/** Activity, or search results for `q` (M75), on the platform page. */
export function openChatView(view: "activity" | "saved" | "search" | "threads", q = "") {
  closeThread();
  openPlatform(view === "search" ? `/chat/search${q ? `?q=${encodeURIComponent(q)}` : ""}` : `/chat/${view}`);
}

/** The platform page's places the shell's buttons hand over (`/?to=`): the
 * platform page opens each in the current project, which only it knows
 * (another origin, Arugula #880), or the global one without. */
export const CHAT_TO = "/?to=chat";
export const BOARDS_TO = "/?to=boards";
export { DOCS_TO };
export const PROJECTS_TO = "/?to=projects";
/** The Inbox is across projects: its own address. */
export const INBOX_TO = "/inbox";

/** Chat, on the platform page (M153): the current project's team's. */
export function openChat() {
  closeThread();
  openPlatform(CHAT_TO);
}

/** Boards, on the platform page: the current project's (#339), or every
 * project's. */
export function openBoards() {
  closeThread();
  openPlatform(BOARDS_TO);
}

/** M123: a room in control's chat, at message `msg`, on the platform page. */
export function openRoom(room: string, msg?: number, thread?: number) {
  closeThread();
  const q = [msg !== undefined ? `msg=${msg}` : "", thread !== undefined ? `thread=${thread}` : ""].filter(Boolean).join("&");
  openPlatform(`/chat/${room}${q ? `?${q}` : ""}`);
}

/** Whether this page offers chat: control's (M123), when its `chat` flag
 * is on for this account and control has a platform page to show it on.
 * Since M155 the shell draws no chat page of its own, and no machine keeps
 * threads of its own (M125, S41 decision 7); a pane's thread opens beside
 * it (./threads). */
export function hasChat(): boolean {
  return !!platformOrigin() && !!chatStore();
}

/** Whether to offer Boards: once control has answered for a person signed
 * in to it, whether or not chat is on for them, since a project's boards and
 * issues follow the project (Arugula #746); and to the machine's owner (as
 * Docs). */
export function hasBoards(client: Client): boolean {
  const known = chat.on === true || (chat.on === false && !chat.signedOut);
  return !!platformOrigin() && known && !client.state?.roles;
}

/** Unread messages in the rooms chat lists, and whether one mentions
 * them: for the bar's button. The threads on issues and runs count on
 * their own lists (shared/chat/unread). */
export function chatUnread(): { n: number; mention: boolean } {
  return chatStore()?.unread() ?? { n: 0, mention: false };
}

/** Re-render on any host's change, and on the route's. */
export function useChatTick(client: Client) {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    const offs = [client.subscribe(fn), getFleet()?.subscribe(fn), chat.subscribe(fn)];
    return () => {
      listeners.delete(fn);
      for (const off of offs) off?.();
    };
  }, [client]);
}

// ---- the places: the panes, and Settings over them (Swarm and the Agents page went in Lift 6a: their links land on the panes)

/** Where a person is: the panes, or Settings over them. */
export type Place = "panes" | "settings";

/** The Machines page at the machine `client` shows (its id, or its name
 * where it has none), and at `session`'s card (Arugula #1349): what M111's
 * overview of a machine's sessions was. Null where there's no app to show
 * it (a machine on its own). */
export function machinesAt(client: Client, session?: number): string | null {
  const machine = client.e2e?.daemon.id ?? directory.find(directory.current)?.id ?? (directory.current || undefined);
  const path = machinesPath({ at: "machines", ...(machine ? { show: machine, ...(session !== undefined ? { session } : {}) } : {}) });
  return platformHref(path) ? path : null;
}

/** Opens `machinesAt`; false where there's no app to. */
export function openMachinesAt(client: Client, session?: number): boolean {
  const path = machinesAt(client, session);
  if (!path) return false;
  closeThread();
  openPlatform(path);
  return true;
}

/** An old `#overview` or `#overview=<session>` (M111's overview, retired,
 * Arugula #1349): the Machines page at this machine, at that session's
 * card, in its place; with no app, the panes, at that session. */
export function landOldOverview(client: Client, show: (session: number) => void) {
  const m = /^#overview(?:=(\d+))?$/.exec(location.hash);
  if (!m) return;
  const session = m[1] ? Number(m[1]) : undefined;
  const path = machinesAt(client, session);
  if (path) return landOnPlatform(path);
  history.replaceState(null, "", location.pathname + location.search);
  dispatchEvent(new HashChangeEvent("hashchange"));
  if (session !== undefined && client.state?.sessions.some((s) => s.id === session)) show(session);
}

/** Where the page is now, from the route. */
export function currentPlace(): Place {
  if (settingsRoute()) return "settings";
  return "panes";
}

/** Back to the panes from wherever you are. */
export function closePlace(at: Place) {
  if (at === "settings") closeSettings();
}
