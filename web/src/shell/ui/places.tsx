// The places a person moves between (the panes, the swarm, agents) and
// chat, which is on the platform page (M153): the places bar's Chat opens
// it there. The shell draws no chat page of its own since M155.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { directory } from "../hosts";
import { SWARM_ABOUT, closeSwarm, openSwarm, swarmRoute } from "../swarm/route";
import { agentsRoute, closeAgentsPage, openAgentsPage } from "./agents-route";
import { closeDocsPage, docsRoute, openDocsPage } from "./docs-route";
import { getFleet } from "./hosts";
import { closeThread } from "./threads";
import { chat, chatStore } from "../../shared/chat-store";
import { openPlatform, platformOrigin } from "../platform-links";

// ---- chat, on the platform page

const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
addEventListener("hashchange", changed);

/** Activity, or search results for `q` (M75), on the platform page. */
export function openChatView(view: "activity" | "saved" | "search" | "threads", q = "") {
  closeThread();
  openPlatform(view === "search" ? `/chat/search${q ? `?q=${encodeURIComponent(q)}` : ""}` : `/chat/${view}`);
}

/** Chat, on the platform page (M153). */
export function openChat() {
  closeThread();
  openPlatform("/chat");
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

// ---- the places: the panes, the swarm and agents

/** Where a person is: the panes, the swarm or agents, or (in the new
 * workspace, M111) the overview of this machine's sessions. */
export type Place = "panes" | "swarm" | "agents" | "docs" | "overview";

/** `#overview`, or `#overview=<session>` for one session's (M111). */
export function overviewRoute(): { session?: number } | null {
  const m = /^#overview(?:=(\d+))?$/.exec(location.hash);
  return m ? { session: m[1] ? Number(m[1]) : undefined } : null;
}

export function openOverview(session?: number) {
  closeThread();
  const hash = session === undefined ? "overview" : `overview=${session}`;
  if (location.hash !== `#${hash}`) location.hash = hash;
}

/** Back to the panes; a hashchange, so everything that follows the route
 * hears it. */
export function closeOverview() {
  if (!overviewRoute()) return;
  history.replaceState(null, "", location.pathname + location.search);
  dispatchEvent(new HashChangeEvent("hashchange"));
}

/** Where the page is now, from the route. */
export function currentPlace(): Place {
  if (overviewRoute()) return "overview";
  if (swarmRoute()) return "swarm";
  if (agentsRoute()) return "agents";
  if (docsRoute()) return "docs";
  return "panes";
}

/** Back to the panes from wherever you are. */
export function closePlace(at: Place) {
  if (at === "agents") closeAgentsPage();
  else if (at === "docs") closeDocsPage();
  else if (at === "swarm") closeSwarm();
  else if (at === "overview") closeOverview();
}

/** Panes · Swarm · Chat · Agents · Docs, in the bar every place shares
 * (./app-bar). Chat opens the platform page. Agents (#403) is the machine
 * owner's, with its flag, and so is Docs (#1494; no flag since native
 * #204). */
export function Places({ client, at }: { client: Client; at: Place }) {
  useChatTick(client);
  const { n, mention } = chatUnread();
  const agents = client.flag("agent-delegation") && !client.state?.roles;
  const docs = !client.state?.roles;
  // No machine yet: no panes or swarm to go to.
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
      {machines && (
        <button
          class={at === "swarm" ? "place here" : "place"}
          title={`Swarm: ${SWARM_ABOUT}`}
          data-open-swarm
          aria-pressed={at === "swarm"}
          onClick={() => at !== "swarm" && openSwarm()}
        >
          Swarm
        </button>
      )}
      {hasChat() && (
        <button
          class="place"
          title="Chat: your teams' channels, messages and threads, on the platform page"
          data-open-chat
          onClick={() => openChat()}
        >
          Chat
          {n > 0 && <span class={mention ? "chat-count mention" : "chat-count"}>{mention ? `@${n}` : n}</span>}
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
      {docs && (
        <button
          class={at === "docs" ? "place here" : "place"}
          title="Docs: the Docs your agents wrote, to open again"
          data-open-docs
          aria-pressed={at === "docs"}
          onClick={() => at !== "docs" && openDocsPage()}
        >
          Docs
        </button>
      )}
    </div>
  );
}
