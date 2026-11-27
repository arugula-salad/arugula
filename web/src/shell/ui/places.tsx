// The places a person moves between (the panes, agents) and
// chat, which is on the platform page (M153): the places bar's Chat opens
// it there. The shell draws no chat page of its own since M155.

import { closeSettings, settingsRoute } from "./settings-route";
import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { directory } from "../hosts";
import { agentsRoute, closeAgentsPage, openAgentsPage } from "./agents-route";
import { DOCS_TO } from "./docs-route";
import { getFleet } from "./hosts";
import { closeThread } from "./threads";
import { chat, chatStore } from "../../shared/chat-store";
import { landOnPlatform, openPlatform, platformHref, platformOrigin } from "../platform-links";
import { machinesPath } from "../../shared/machine-paths";
import { Count } from "../../shared/ui/state-chip";

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
export const RUNS_TO = "/?to=runs";
/** The Inbox is across projects: its own address. */
export const INBOX_TO = "/inbox";
/** The Machines page in the app (Arugula #1346), which only the app has. */
const MACHINES_PAGE = machinesPath({ at: "machines" });

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

// ---- the places: the panes and agents (Swarm went in Lift 6a: `#swarm` lands on the panes)

/** Where a person is: the panes or agents. */
export type Place = "panes" | "agents" | "settings";

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
  if (agentsRoute()) return "agents";
  if (settingsRoute()) return "settings";
  return "panes";
}

/** Back to the panes from wherever you are. */
export function closePlace(at: Place) {
  if (at === "agents") closeAgentsPage();
  else if (at === "settings") closeSettings();
}

/** Panes · Chat · Boards · Docs · Agents, in the bar every place
 * shares (./app-bar). Chat, Boards and Docs open the platform page. Agents
 * (#403) is the machine owner's, with its flag. */
export function Places({ client, at }: { client: Client; at: Place }) {
  useChatTick(client);
  const { n, mention } = chatUnread();
  const agents = client.flag("agent-delegation") && !client.state?.roles;
  const docs = !client.state?.roles && !!platformHref(DOCS_TO);
  // No machine yet: no panes to go to.
  const machines = !!client.state || directory.names.length > 0;
  return (
    <div class="places" role="group" aria-label="Places">
      {machines && (
        <button
          class={at === "panes" ? "place here" : "place"}
          title="Your panes"
          data-open-panes
          aria-pressed={at === "panes"}
          onClick={() => closePlace(at)}
        >
          Panes
        </button>
      )}
      {hasChat() && (
        <button
          class="place"
          title="Chat: your teams' channels, messages and threads"
          data-open-chat
          onClick={() => openChat()}
        >
          Chat
          {n > 0 && <Count for={mention ? "mention" : "you"} class="count-after">{mention ? `@${n}` : n}</Count>}
        </button>
      )}
      {hasBoards(client) && (
        <button
          class="place"
          title="Boards: your projects' boards and epics"
          data-open-boards
          onClick={() => openBoards()}
        >
          Boards
        </button>
      )}
      {docs && (
        <button class="place" title="Docs: your project's Docs" data-open-docs onClick={() => openPlatform(DOCS_TO)}>
          Docs
        </button>
      )}
      {agents && (
        <button
          class={at === "agents" ? "place here" : "place"}
          title="Agents: this machine's recipes and your team's agents"
          data-open-agents
          aria-pressed={at === "agents"}
          onClick={() => at !== "agents" && openAgentsPage()}
        >
          Agents
        </button>
      )}
    </div>
  );
}

/** Machines' views, in a machine's bar on a computer (Arugula #637,
 * #1345): the bar is what you're in, the machine, then Machines and Runs, as
 * the app's Machines are. Machines is the app's Machines page, and
 * Runs its runs, where control has a platform page; a machine on its own
 * keeps its Panes and Swarm (its Docs are in Go to…; the app's Docs are
 * the rail's). Recipes is this machine's. */
export function MachinePlaces({ client, at }: { client: Client; at: Place }) {
  const owner = !client.state?.roles;
  const recipes = client.flag("agent-delegation") && owner;
  const panes = at === "panes";
  const app = !!platformHref(MACHINES_PAGE);
  const runs = !!platformHref(RUNS_TO);
  const item = (name: string, hook: string, here: boolean, title: string, go: () => void) => (
    <button
      class={here ? "place frame-place here" : "place frame-place"}
      title={title}
      aria-current={here ? "page" : undefined}
      {...{ [`data-machine-${hook}`]: "" }}
      onClick={go}
    >
      {name}
    </button>
  );
  const back = () => {
    if (at !== "panes") closePlace(at);
    else if (app) openPlatform(MACHINES_PAGE);
    else client.viewOf(client.active() ?? -1)?.focus();
  };
  return (
    <nav class="places" aria-label="Machines' views" data-machine-places>
      {/* In the app, Machines is the Machines page (Arugula #1346), a step up from here. */}
      {app
        ? item("Machines", "machines", panes, "Machines: every machine's sessions and panes", back)
        : item("Panes", "panes", panes, "Your panes: back to this session's last tab and pane", back)}
      {runs && item("Runs", "runs", false, "Runs: your project's runs", () => openPlatform(RUNS_TO))}
      {recipes && item("Recipes", "recipes", at === "agents", "Recipes: this machine's recipes and your team's agents", () => at !== "agents" && openAgentsPage())}
    </nav>
  );
}
