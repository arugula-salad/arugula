// The places a person moves between (the panes, the swarm, chat) and chat's
// route in the URL hash. Core: the panes' bar shows the places, and the page
// has to know whether chat is open, with or without Labs. The chat page itself
// (./chat) is Labs and loads only with it.

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import type { Fleet } from "../fleet";
import { directory } from "../hosts";
import { openSwarm } from "../swarm/route";
import { threadKey, type ThreadTarget } from "../proto";
import { getFleet } from "./hosts";
import { closeThread } from "./threads";

// ---- the route

const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
addEventListener("hashchange", changed);

export interface ChatRoute {
  host?: string;
  key?: string;
  /** M74: a message to show (a copied link). */
  msg?: number;
  /** M75: Activity (your mentions), or search results for `q`. */
  view?: "activity" | "search";
  q?: string;
}

/** `#chat`, `#chat=<host>/<pane-N|session-N>` for one thread, and
 * `…&msg=N` for one message in it; `#chat=activity`, and
 * `#chat=search/<words>`. */
export function chatRoute(): ChatRoute | null {
  if (location.hash === "#chat=activity") return { view: "activity" };
  const q = /^#chat=search\/(.*)$/.exec(location.hash);
  if (q) return { view: "search", q: safeDecode(q[1]) };
  const m = /^#chat(?:=(.*)\/((?:pane|session)-\d+)(?:&msg=(\d+))?)?$/.exec(location.hash);
  if (!m) return null;
  return m[2] ? { host: safeDecode(m[1]), key: m[2], msg: m[3] ? Number(m[3]) : undefined } : {};
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

export function openChat(host?: string, target?: ThreadTarget, msg?: number) {
  closeThread();
  const hash =
    host !== undefined && target ? `chat=${encodeURIComponent(host)}/${threadKey(target)}${msg !== undefined ? `&msg=${msg}` : ""}` : "chat";
  if (location.hash !== `#${hash}`) location.hash = hash;
  else changed();
}

export function closeChat() {
  history.replaceState(null, "", location.pathname + location.search);
  changed();
}

export function isChatOpen(): boolean {
  return chatRoute() !== null;
}

export function targetOf(key: string): ThreadTarget | null {
  const m = /^(pane|session)-(\d+)$/.exec(key);
  if (!m) return null;
  return m[1] === "pane" ? { pane: Number(m[2]) } : { session: Number(m[2]) };
}

// ---- where the threads come from

export interface Source {
  host: string;
  client: Client;
  /** The page's own connection (it can show panes; the rest go there). */
  shown: boolean;
}

/** Every host with threads to show: the shown one through the page's own
 * client, the others through the fleet's, when they keep threads. */
export function sources(client: Client, fleet: Fleet | null): Source[] {
  const current = directory.current;
  const out: Source[] = [{ host: current, client, shown: true }];
  for (const h of fleet?.list ?? []) {
    if (h.name === current) continue;
    const c = fleet!.clientOf(h.name);
    if (c?.state && c.hasThreads()) out.push({ host: h.name, client: c, shown: false });
  }
  return out;
}

/** Unread messages in every thread this person sees, and whether one
 * mentions them: for the bar's button. */
export function chatUnread(client: Client): { n: number; mention: boolean } {
  let n = 0;
  let mention = false;
  for (const s of sources(client, getFleet())) {
    for (const t of s.client.state?.threads ?? []) {
      n += t.unread ?? 0;
      mention ||= !!t.mention;
    }
  }
  return { n, mention };
}

/** Re-render on any host's change, and on the route's. */
export function useChatTick(client: Client) {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    const offs = [client.subscribe(fn), getFleet()?.subscribe(fn)];
    return () => {
      listeners.delete(fn);
      for (const off of offs) off?.();
    };
  }, [client]);
}

// ---- the places: the panes, the swarm and chat

/** Panes · Swarm · Chat, in the panes' bar and in chat's. */
export function Places({ client, at }: { client: Client; at: "panes" | "chat" }) {
  useChatTick(client);
  const { n, mention } = client.hasThreads() ? chatUnread(client) : { n: 0, mention: false };
  return (
    <div class="places" role="group" aria-label="Places">
      <button class={at === "panes" ? "place here" : "place"} title="Your panes" data-open-panes aria-pressed={at === "panes"} onClick={() => at === "chat" && closeChat()}>
        Panes
      </button>
      <button class="place" title="Every pane, everywhere (the swarm)" data-open-swarm onClick={openSwarm}>
        Swarm
      </button>
      {client.hasThreads() && (
        <button
          class={at === "chat" ? "place here" : "place"}
          title="Chat: every thread, on every machine"
          data-open-chat
          aria-pressed={at === "chat"}
          onClick={() => at === "panes" && openChat()}
        >
          Chat
          {n > 0 && <span class={mention ? "chat-count mention" : "chat-count"}>{mention ? `@${n}` : n}</span>}
        </button>
      )}
    </div>
  );
}
