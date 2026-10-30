// The page's live presence store (./store, #343), wired: arugula.stream's
// SDK from the relay's site, room tokens from control
// (`POST /api/chat/presence/token`, a team's members only), the switch
// (./switch), the window's visibility, and the reader's teams as chat
// knows them (every room's team), the same on control's page and the
// platform page.

import { chat } from "../chat-store";
import { platformFetch, refusalOf } from "../maintenance";
import { LiveStore, Refused, type Sdk, type Token } from "./store";
import { liveState, subscribeLive } from "./switch";

export const TOKEN_URL = "/api/chat/presence/token";

/** The relay's https origin, from its socket's URL. */
export function httpOrigin(relay: string): string {
  return new URL(relay.replace(/^ws/, "http")).origin;
}

/** arugula.stream's SDK (its elements too), loaded once from the relay's site. */
let loaded: Promise<Sdk> | null = null;
export function loadSdk(relay: string): Promise<Sdk> {
  loaded ??= (import(/* @vite-ignore */ `${httpOrigin(relay)}/sdk/v0/presence.js`) as Promise<Sdk>).catch((e) => {
    loaded = null;
    throw e;
  });
  return loaded;
}

/** A room token for `team:<team>`, or why not: control's refusal (no app,
 * not a member, chat off) is final; anything else may pass. */
export async function roomToken(team: string): Promise<Token> {
  const r = await platformFetch(TOKEN_URL, {
    method: "POST",
    credentials: "same-origin",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ room: `team:${team}` }),
  });
  const body = (await r.json().catch(() => ({}))) as { token?: string; relay?: string; error?: string; reason?: string };
  if (r.ok && body.token && body.relay) return { token: body.token, relay: body.relay };
  const why = refusalOf(r.status, body, `Control said ${r.status}`);
  throw r.status >= 400 && r.status < 500 ? new Refused(why) : new Error(why);
}

export const live = new LiveStore({ state: liveState, subscribe: subscribeLive, token: roomToken, sdk: loadSdk });

if (typeof document !== "undefined") {
  const seen = () => live.setHidden(document.visibilityState === "hidden");
  addEventListener("visibilitychange", seen);
  seen();
  // The reader's teams: every chat room's. A team with chat off has none,
  // and its room would be refused anyway.
  const teams = () => live.setTeams(chat.rooms.map((r) => r.team).filter((t): t is string => !!t));
  chat.subscribe(teams);
  teams();
}

/** The team Live's camera goes to: the page says which it's working in. */
export function setWorkingTeam(team: string | null) {
  live.setWorking(team);
}
