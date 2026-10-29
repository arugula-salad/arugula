// meet's side of the relay: its JSON under /api/v1/meet (by session
// cookie), signing in and out (P3's /api/v1/auth and /auth pages), and
// where this page lives (meet.arugula.stream/, or /meet on any host).

import type { Look } from "@arugula/presence-core/avatar/rig";

/** The app's base: "/meet" when it's served under /meet, else "". */
export const BASE = /^\/meet(\/|$)/.test(location.pathname) ? "/meet" : "";

export const path = (p: string) => `${BASE}${p}`;

/** Goes to a path here without reloading the page. */
export function go(to: string) {
  history.pushState(null, "", to);
  dispatchEvent(new PopStateEvent("popstate"));
}

export interface Account {
  id: string;
  email: string | null;
  name: string | null;
}

export interface Session {
  account: Account | null;
  /** meet's publishable key, for guests. */
  key: string | null;
  sign_in: { email: boolean; google: boolean };
}

export interface RoomInfo {
  slug: string;
  name: string;
  created_at: string;
  mine: boolean;
  joined_at?: string;
}

async function call<T>(method: string, url: string, body?: unknown): Promise<T> {
  const r = await fetch(url, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = (await r.json().catch(() => ({}))) as T & { error?: string };
  if (!r.ok) throw new ApiError(r.status, json.error ?? `the server said ${r.status}`);
  return json;
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

export const session = () => call<Session>("GET", "/api/v1/meet/session");
export const rooms = () => call<{ made: RoomInfo[]; recent: RoomInfo[] }>("GET", "/api/v1/meet/rooms");
export const createRoom = (name: string) => call<{ room: RoomInfo }>("POST", "/api/v1/meet/rooms", { name }).then((r) => r.room);
export const room = (slug: string) => call<{ room: RoomInfo }>("GET", `/api/v1/meet/rooms/${encodeURIComponent(slug)}`).then((r) => r.room);

/** A signed-in person's room token (it also puts the room in their list). */
export const roomToken = (slug: string, name: string, look: Look) =>
  call<{ token: string; sub: string }>("POST", `/api/v1/meet/rooms/${encodeURIComponent(slug)}/token`, { name, look });

/** Emails a sign-in link that comes back to `returnTo` (a path here). */
export const emailLink = (email: string, returnTo: string) => call<{ sent: true }>("POST", "/api/v1/auth/email", { email, return_to: returnTo });

export const signOut = () => call<{ signed_out: true }>("POST", "/api/v1/auth/logout", {});

export const googleUrl = (returnTo: string) => `/auth/google?return_to=${encodeURIComponent(returnTo)}`;

/** What to call a signed-in person until they say: their name, or their
 * address's first part. */
export const accountName = (a: Account) => a.name || (a.email ?? "").split("@")[0] || "";

export const NAME = "meet.name";

export function kept(key: string): string {
  try {
    return localStorage.getItem(key) ?? "";
  } catch {
    return "";
  }
}

export function keep(key: string, v: string) {
  try {
    localStorage.setItem(key, v);
  } catch {
    // This page still has it.
  }
}
