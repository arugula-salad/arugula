// The places a person moves between (the panes, the swarm, chat, agents) and chat's
// route in the URL hash. Core: the panes' bar shows the places, and the page
// has to know whether chat is open, with or without Labs. The chat page itself
// (./chat) is Labs and loads only with it.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import { directory } from "../hosts";
import { closeSwarm, openSwarm, swarmRoute } from "../swarm/route";
import { agentsRoute, closeAgentsPage, openAgentsPage } from "./agents-route";
import { getFleet } from "./hosts";
import { closeThread } from "./threads";
import { chat, chatStore, ROOM_ID } from "../chat-store";

// ---- the route

const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
addEventListener("hashchange", changed);

export interface ChatRoute {
  /** M74: a message to show (a copied link). */
  msg?: number;
  /** M75: Activity (your mentions), or search results for `q`. */
  view?: "activity" | "search";
  q?: string;
  /** M123: a room in control's chat (`#chat=<room>`). */
  room?: string;
}

/** `#chat`, `#chat=<room>` for one room of control's chat and `…&msg=N`
 * for one message in it; `#chat=activity`, and `#chat=search/<words>`. */
export function chatRoute(): ChatRoute | null {
  if (location.hash === "#chat=activity") return { view: "activity" };
  const q = /^#chat=search\/(.*)$/.exec(location.hash);
  if (q) return { view: "search", q: safeDecode(q[1]) };
  // M123: a room in control's chat, and a message in it (imported ones
  // are numbered 0 and below).
  const r = /^#chat=([a-z]{2}_[0-9a-f]+)(?:&msg=(-?\d+))?$/.exec(location.hash);
  if (r && ROOM_ID.test(r[1])) return { room: r[1], msg: r[2] ? Number(r[2]) : undefined };
  // Anything else of chat's (a link to a machine's thread from before
  // M125): chat, from the top.
  return /^#chat(?:=|$)/.test(location.hash) ? {} : null;
}

function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

/** Activity, or search results for `q` (M75). */
export function openChatView(view: "activity" | "search", q = "") {
  closeThread();
  const hash = view === "activity" ? "chat=activity" : `chat=search/${encodeURIComponent(q)}`;
  if (location.hash === `#${hash}`) return changed();
  // Typing in search replaces the entry, so Back doesn't step through it.
  if (view === "search" && chatRoute()?.view === "search") {
    history.replaceState(null, "", `#${hash}`);
    changed();
  } else location.hash = hash;
}

/** Whether the chat page is shown, kept current with the route. */
export function useChatOpen(): boolean {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  return isChatOpen();
}

export function openChat() {
  closeThread();
  if (location.hash !== "#chat") location.hash = "chat";
  else changed();
}

/** M123: a room in control's chat, at message `msg`. */
export function openRoom(room: string, msg?: number) {
  closeThread();
  const hash = `chat=${room}${msg !== undefined ? `&msg=${msg}` : ""}`;
  if (location.hash !== `#${hash}`) location.hash = hash;
  else changed();
}

/** Whether this page has chat: control's (M123), when its `chat` flag is
 * on for this account. No machine keeps threads of its own (M125). */
export function hasChat(): boolean {
  return !!chatStore();
}

export function closeChat() {
  history.replaceState(null, "", location.pathname + location.search);
  changed();
}

export function isChatOpen(): boolean {
  return chatRoute() !== null;
}

/** Unread messages in every thread this person sees, and whether one
 * mentions them: for the bar's button. */
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

// ---- the places: the panes, the swarm, chat and agents

/** Where a person is: the panes, the swarm, chat or agents, or (in the new
 * workspace, M111) the overview of this machine's sessions. */
export type Place = "panes" | "swarm" | "chat" | "agents" | "overview";

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
  if (isChatOpen()) return "chat";
  if (swarmRoute()) return "swarm";
  if (agentsRoute()) return "agents";
  return "panes";
}

/** Back to the panes from wherever you are. */
export function closePlace(at: Place) {
  if (at === "chat") closeChat();
  else if (at === "agents") closeAgentsPage();
  else if (at === "swarm") closeSwarm();
  else if (at === "overview") closeOverview();
}

/** Panes · Swarm · Chat · Agents, in the bar every place shares
 * (./app-bar). Agents (#403) is the machine owner's, with its flag. */
export function Places({ client, at }: { client: Client; at: Place }) {
  useChatTick(client);
  const { n, mention } = chatUnread();
  const agents = client.flag("agents") && !client.state?.roles;
  // M123: control's chat for someone with no machine: no panes to go to.
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
          title="Every pane, everywhere (the swarm)"
          data-open-swarm
          aria-pressed={at === "swarm"}
          onClick={() => at !== "swarm" && openSwarm()}
        >
          Swarm
        </button>
      )}
      {!machines && (
        <button class="place" title="Back to adding a machine" data-chat-leave onClick={() => closePlace(at)}>
          Machines
        </button>
      )}
      {hasChat() && (
        <button
          class={at === "chat" ? "place here" : "place"}
          title="Chat: your teams' channels, messages and threads"
          data-open-chat
          aria-pressed={at === "chat"}
          onClick={() => at !== "chat" && openChat()}
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
    </div>
  );
}
