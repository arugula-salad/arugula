// Machine tokens (aru-141): control's word for who may reach a machine, in
// place of this browser's device key. A channel opened with one carries
// `{"machine_token": "<jwt>"}` (control-wire's `TokenHello`) in its first
// Noise message, under a static key made for that channel alone: the
// daemon checks the token and ignores the key. The daemon takes each
// token once, so every try that sends message 1 asks control for a new one
// (`POST /api/machines/{id}/token`, signed in by the session cookie).

import type { Cert } from "./cert.ts";
import type { TeamRole } from "./team.ts";
import type { DirDaemon, ForeignEntry } from "../control.ts";

const enc = new TextEncoder();

/** Where a channel's machine tokens come from. */
export interface MachineTokens {
  /** A token for machine `id`, fresh: never one a channel has sent. */
  token(id: string): Promise<string>;
}

/** The first message's payload for `token`. */
export function tokenHello(token: string): Uint8Array<ArrayBuffer> {
  return enc.encode(JSON.stringify({ machine_token: token }));
}

/** The refusal code a daemon answers a token it won't take with
 * (expired, already used, another machine's). */
export const TOKEN_REFUSED = "machine-token";

/** Control couldn't give a token: signed out, no longer let in to the
 * machine, or a control from before machine tokens. No way to the machine
 * would do better, so the channel gives up until its next try. */
export class NoTokenError extends Error {
  readonly noToken = true;
  readonly status: number;
  /** Seconds the hub said to wait (`Retry-After`), if it said. */
  readonly retryAfter: number | undefined;
  /** Set by `gatedTokens`: how long before a try could get a token, ms. */
  retryMs: number | undefined;
  constructor(status: number, message: string, retryAfter?: number) {
    super(message);
    this.status = status;
    this.retryAfter = retryAfter;
  }
}

/** The least a page waits after control refuses a token (429 or worse). */
export const TOKEN_BACKOFF_MS = 60_000;
const TOKEN_BACKOFF_MAX_MS = 15 * 60_000;

/** `tokens`, asked with care (the 2026-11-14 outage: pages retrying offline
 * machines spent the account's 3,000 tokens an hour, and every slot freed
 * went to the next retry). Nothing is asked for a machine `online` says is
 * offline; one ask per machine at a time; a 429 stops every machine's asks
 * on this page for `Retry-After`, else a minute and growing; any other
 * refusal (but 404, which says there are no tokens here) stops that
 * machine's the same way. A try held back fails at once, with `retryMs`. */
export function gatedTokens(tokens: MachineTokens, online: (id: string) => boolean | undefined = () => undefined, now: () => number = () => Date.now()): MachineTokens {
  let account = { until: 0, strikes: 0 };
  const machines = new Map<string, { until: number; strikes: number }>();
  const asking = new Map<string, Promise<unknown>>();
  const held = (until: number, msg: string, status: number) => {
    const e = new NoTokenError(status, msg);
    e.retryMs = Math.max(1000, until - now());
    return e;
  };
  const limiting = (until: number) => `Arugula cloud is limiting connections; trying again in ${Math.ceil(Math.max(0, until - now()) / 1000)} s`;
  const later = (b: { until: number; strikes: number }, e: NoTokenError) => {
    const wait = Math.min(Math.max((e.retryAfter ?? 0) * 1000, TOKEN_BACKOFF_MS * 2 ** b.strikes), TOKEN_BACKOFF_MAX_MS);
    return { until: now() + wait, strikes: b.strikes + 1 };
  };
  return {
    async token(id) {
      const before = asking.get(id);
      let done!: () => void;
      const mine = new Promise<void>((r) => (done = r));
      asking.set(id, mine);
      await before;
      try {
        if (now() < account.until) throw held(account.until, limiting(account.until), 429);
        const m = machines.get(id);
        if (m && now() < m.until) throw held(m.until, `Arugula cloud gave no token for this machine; trying again in ${Math.ceil((m.until - now()) / 1000)} s`, 0);
        if (online(id) === false) throw held(now() + 5000, "This machine is offline", 0);
        try {
          const t = await tokens.token(id);
          account = { until: 0, strikes: 0 };
          machines.delete(id);
          return t;
        } catch (e) {
          if (!(e instanceof NoTokenError) || e.status === 404) throw e;
          if (e.status === 429) {
            account = later(account, e);
            throw held(account.until, limiting(account.until), 429);
          }
          const b = later(m ?? { until: 0, strikes: 0 }, e);
          machines.set(id, b);
          e.retryMs = b.until - now();
          throw e;
        }
      } finally {
        done();
        if (asking.get(id) === mine) asking.delete(id);
      }
    },
  };
}

/** Tokens from control at `origin` ("" for this page's own): what the page,
 * the hand and the service worker all ask. */
export function controlTokens(origin = "", send: typeof fetch = (i, o) => fetch(i, o)): MachineTokens {
  return {
    async token(id) {
      const res = await send(`${origin}/api/machines/${encodeURIComponent(id)}/token`, {
        method: "POST",
        credentials: "same-origin",
        headers: { "Content-Type": "application/json" },
        body: "{}",
      });
      const j = (await res.json().catch(() => ({}))) as { token?: unknown; error?: unknown; reason?: unknown };
      if (!res.ok || typeof j.token !== "string" || !j.token) {
        const said = typeof j.error === "string" ? j.error : typeof j.reason === "string" ? j.reason : `HTTP ${res.status}`;
        const after = Number(res.headers?.get?.("Retry-After"));
        throw new NoTokenError(res.status, `Arugula cloud gave no token for this machine: ${said}`, Number.isFinite(after) && after > 0 ? after : undefined);
      }
      return j.token;
    },
  };
}

/** Whether control at `origin` signs machine tokens: it publishes its
 * issuer's keys. A control without an issuer (a self-host or a test hub
 * started without one) answers 404, and a browser that isn't a device yet
 * asks to be approved instead of waiting on tokens that never come. */
export async function controlIssues(origin = "", send: typeof fetch = (i, o) => fetch(i, o)): Promise<boolean> {
  try {
    const res = await send(`${origin}/.well-known/jwks.json`, { credentials: "omit" });
    if (!res.ok) return false;
    const j = (await res.json()) as { keys?: unknown };
    return Array.isArray(j.keys) && j.keys.length > 0;
  } catch {
    return false;
  }
}

/** How this browser reaches its machines: `first` (the default since
 * every machine runs 0.70.4 or newer, Jake 2026-11-14) uses tokens even
 * when it's one of the account's devices; `no-key` uses them only when it
 * isn't; `off` never does, and a browser that isn't a device asks to be
 * approved, as before machine tokens. */
export type TokenMode = "no-key" | "first" | "off";

/** Where a person (or a test) can say which, until the device path goes
 * (#1004): no Labs flag, since it's about this browser, not the account. */
export const TOKEN_MODE_KEY = "arugula.machine-tokens";

/** The default for every browser that hasn't said. */
export const TOKEN_MODE_DEFAULT: TokenMode = "first";

export function tokenMode(read: () => string | null = () => localStorage.getItem(TOKEN_MODE_KEY)): TokenMode {
  try {
    const v = read();
    if (v === "first" || v === "off" || v === "no-key") return v;
  } catch {
    // no storage here
  }
  return TOKEN_MODE_DEFAULT;
}

/** A directory entry as a browser with machine tokens takes it: its Noise
 * key as control gives it (#994), else from the certificate listed beside
 * it (a hosted machine's, another account's chain, or the account's own
 * `certs`). With none, it can't be reached from here, and is left out.
 * Nothing is checked against a root: control decides who reaches it. A
 * hosted machine is reached through the relay only, as from a device. */
export function tokenEntry(d: ForeignEntry, certs: Cert[], account: string, teams: { team: string; roster: { members: { account: string; role: TeamRole }[] } }[]): DirDaemon | null {
  const { chain, hosted: proof, noise: given, ...entry } = d;
  const listed = proof ? undefined : [...(chain?.certs ?? []), ...certs].find((c) => c.device === d.id && c.kind === "daemon");
  const noise = (given ?? proof?.cert.noise ?? listed?.noise)?.toLowerCase();
  if (!noise || !/^[0-9a-f]{64}$/.test(noise)) return null;
  const cert: Cert = listed
    ? { ...listed, noise }
    : { v: 1, account: proof?.cert.project ?? d.account ?? account, device: d.id, kind: "daemon", name: d.name, noise, sign: proof?.cert.sign.toLowerCase() ?? "", created: proof?.cert.created ?? 0, approver: "", sig: "" };
  if (!proof) return { ...entry, cert };
  // The role it's shown with, as the project's owner names it; the machine
  // goes by the one in each token, which control decides.
  const owner = proof.delegation.owner;
  const team = owner.startsWith("team:") ? teams.find((t) => `team:${t.team}` === owner) : undefined;
  const role: TeamRole = owner === `account:${account}` ? "owner" : (team?.roster.members.find((m) => m.account === account)?.role ?? "viewer");
  return { ...entry, urls: [], cert, hosted: { project: proof.cert.project, workspace: proof.cert.workspace, role } };
}
