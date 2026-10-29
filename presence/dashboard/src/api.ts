// The dashboard's side of the relay: who's signed in (/api/v1/me), the
// ways in (P3's /api/v1/auth and /auth pages), and the owner's apps under
// /api/v1/apps (keys, origins, guests, the server secret, the sign-in
// provider with its room rule, and usage from #318's P9). Every write is
// JSON, as the relay insists.

/** Where the dashboard lives: always under /dashboard. */
export const BASE = "/dashboard";

export const path = (p: string) => `${BASE}${p}`;

/** Goes to a path here without reloading the page. */
export function go(to: string) {
  history.pushState(null, "", to);
  dispatchEvent(new PopStateEvent("popstate"));
  scrollTo(0, 0);
}

export interface Account {
  id: string;
  email: string | null;
  name: string | null;
}

export interface Providers {
  email: boolean;
  google: boolean;
}

export interface Key {
  key: string;
  mode: "live" | "test";
  made: string;
}

export interface App {
  id: string;
  name: string;
  guests: boolean;
  origins: string[];
  keys: Key[];
}

export type Provider = "clerk" | "auth0" | "firebase" | "supabase" | "oidc";

export interface RoomRule {
  claim: string;
  match: "prefix" | "contains";
}

/** An app's sign-in provider as the relay shows it, with the `iss` and the
 * key URLs its settings make. */
export interface Issuer {
  provider: Provider;
  issuer: string | null;
  project: string | null;
  audience: string | null;
  name_claim: string;
  room_rule: RoomRule | null;
  iss: string;
  jwks?: string | null;
  discovery?: string | null;
}

/** What PUT /issuer takes. */
export interface IssuerInput {
  provider: Provider;
  issuer?: string;
  project?: string;
  audience?: string;
  name_claim?: string;
  room_rule?: RoomRule | null;
}

export interface Usage {
  months: { month: string; active_users: number; call_minutes: number }[];
  /** The last 31 days, oldest first. */
  days: { day: string; active_users: number; call_minutes: number }[];
  cap: { active_users: number | null };
  this_month: { active_users: number; call_minutes: number };
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
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

const app = (id: string, rest = "") => `/api/v1/apps/${encodeURIComponent(id)}${rest}`;

/** The signed-in account, or null. */
export const me = () =>
  call<{ account: Account }>("GET", "/api/v1/me").then(
    (r) => r.account,
    (e: ApiError) => {
      if (e.status === 401) return null;
      throw e;
    },
  );

export const providers = () => call<Providers>("GET", "/api/v1/auth/providers");

/** Emails a sign-in link that comes back to `returnTo` (a path here). */
export const emailLink = (email: string, returnTo: string) => call<{ sent: true }>("POST", "/api/v1/auth/email", { email, return_to: returnTo });

export const googleUrl = (returnTo: string) => `/auth/google?return_to=${encodeURIComponent(returnTo)}`;

export const signOut = () => call<{ signed_out: true }>("POST", "/api/v1/auth/logout", {});

export const apps = () => call<{ apps: App[] }>("GET", "/api/v1/apps").then((r) => r.apps);

/** A new app, its first live key, and its server secret (shown this once). */
export const createApp = (name: string) => call<{ app: App; secret: string }>("POST", "/api/v1/apps", { name });

export const getApp = (id: string) => call<{ app: App }>("GET", app(id)).then((r) => r.app);

export const updateApp = (id: string, changes: { name?: string; guests?: boolean }) => call<{ app: App }>("PATCH", app(id), changes).then((r) => r.app);

/** A new server secret; the old one stops working. Shown this once. */
export const rotateSecret = (id: string) => call<{ secret: string }>("POST", app(id, "/secret"), {}).then((r) => r.secret);

export const addOrigin = (id: string, origin: string) => call<{ app: App }>("POST", app(id, "/origins"), { origin }).then((r) => r.app);

export const removeOrigin = (id: string, origin: string) => call<{ app: App }>("DELETE", app(id, "/origins"), { origin }).then((r) => r.app);

export const makeKey = (id: string, mode: "live" | "test") => call<{ key: Key }>("POST", app(id, "/keys"), { mode }).then((r) => r.key);

export const revokeKey = (id: string, key: string) => call<{ revoked: string }>("DELETE", app(id, `/keys/${encodeURIComponent(key)}`));

export const issuer = (id: string) => call<{ issuer: Issuer | null }>("GET", app(id, "/issuer")).then((r) => r.issuer);

export const putIssuer = (id: string, input: IssuerInput) => call<{ issuer: Issuer }>("PUT", app(id, "/issuer"), input).then((r) => r.issuer);

export const deleteIssuer = (id: string) => call<{ issuer: null }>("DELETE", app(id, "/issuer"));

/** The app's usage, or null where the relay doesn't meter yet (a 404:
 * #318's P9 adds it). */
export const usage = (id: string, months = 6) =>
  call<Usage>("GET", app(id, `/usage?months=${months}`)).catch((e: ApiError) => {
    if (e.status === 404 && e.message !== "no such app") return null;
    throw e;
  });

export const setCap = (id: string, activeUsers: number | null) => call<unknown>("PUT", app(id, "/cap"), { active_users: activeUsers });

/** What to call a signed-in person: their name, or their address's first
 * part (without a +tag). */
export const accountName = (a: Account) => a.name || (a.email ?? "").split("@")[0]!.split("+")[0] || "there";
