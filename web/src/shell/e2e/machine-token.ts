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
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
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
        throw new NoTokenError(res.status, `Arugula cloud gave no token for this machine: ${said}`);
      }
      return j.token;
    },
  };
}

/** How this browser reaches its machines: `no-key` (the default) uses
 * tokens only when it isn't one of the account's devices; `first` uses
 * them even when it is; `off` never does, and a browser that isn't a
 * device asks to be approved, as before machine tokens. */
export type TokenMode = "no-key" | "first" | "off";

/** Where a person (or a test) can say which, until the device path goes
 * (#1004): no Labs flag, since it's about this browser, not the account. */
export const TOKEN_MODE_KEY = "arugula.machine-tokens";

/** The default for every browser that hasn't said. */
export const TOKEN_MODE_DEFAULT: TokenMode = "no-key";

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
