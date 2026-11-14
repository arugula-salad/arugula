// The app's way to its machines (Arugula #989): what control's session is
// to control's page, for the page on app.arugula.io, without device keys
// (aru-141). It lists the machines this person reaches from the hub's
// directory, opens channels to them with machine tokens (Jake's
// `controlTokens`, from the page's own origin) through the relay on the
// page's own origin, and keeps the account's prefs.
//
// The hub serves the directory, the tokens and the relay on the app's host
// only once the cloud lets them through there. Until then the directory or
// a token answers 404: `reachable` goes false, and the app keeps today's
// links to control's page (`fallback`). Blocks draw in their own origins
// under the hub's block domain, which `/api/app/config` says.

import type { E2ETarget } from "../shell/client";
import type { DirDaemon, ForeignEntry, Phase, Team } from "../shell/control";
import { controlTokens, gatedTokens, NoTokenError, tokenEntry, type MachineTokens } from "../shell/e2e/machine-token.ts";
import type { MachineSource } from "../shell/machines";

type Send = typeof fetch;

/** How often the machine list is read again, as control's page does. */
export const REFRESH_MS = 10_000;

export class AppSession implements MachineSource {
  /** `loading` until the directory first answers; `ready` once it has, or
   * once it's known not to be here (then `reachable` is false). */
  phase: Phase = "loading";
  daemons: DirDaemon[] = [];
  /** The last read failed: the machines are as they were, and offline. */
  stale = false;
  /** Signed rosters are the device path's; the app names teams from the
   * platform's projects, so the fleet gets none from here. */
  teams: Team[] = [];
  login = "";
  /** Whether the hub reaches machines from this host: null until known. */
  reachable: boolean | null = null;
  /** Where blocks get origins of their own (`b-<site_key>.<this>`), as
   * the hub's `/api/app/config` says; null from a hub that serves none, or
   * one from before it said (blocks then don't draw here). */
  blockDomain: string | null = null;
  private listeners = new Set<() => void>();
  private tokens: MachineTokens;
  private timer: ReturnType<typeof setInterval> | undefined;
  private readonly account: () => { account: string; login: string } | null;
  private readonly control: string;
  private readonly origin: string;
  private readonly send: Send;

  /** `account`: who's signed in, as the platform says (`session.me`).
   * `control`: control's page, for today's links. `origin`: this page's,
   * which the relay and the tokens are on. */
  constructor(account: () => { account: string; login: string } | null, control: string, origin = location.origin, send: Send = (i, o) => fetch(i, o)) {
    this.account = account;
    this.control = control;
    this.origin = origin;
    this.send = send;
    const fetched = controlTokens("", send);
    const given = gatedTokens(fetched, (id) => this.daemons.find((d) => d.id === id)?.online);
    this.tokens = {
      token: async (id) => {
        try {
          return await given.token(id);
        } catch (e) {
          // No token route on this host: the hub can't reach machines here.
          if (e instanceof NoTokenError && e.status === 404) this.unreachable();
          throw e;
        }
      },
    };
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => void this.listeners.delete(fn);
  }

  private emit() {
    for (const fn of [...this.listeners]) fn();
  }

  private unreachable() {
    if (this.reachable === false) return;
    this.reachable = false;
    this.phase = "ready";
    this.emit();
  }

  /** Read the hub's config, then the machine list, and the list again
   * every `REFRESH_MS`; the answer stops. The config comes first, so every
   * target the page makes has its block domain. */
  start(): () => void {
    void this.config().then(() => this.refresh());
    this.timer ??= setInterval(() => void this.refresh(), REFRESH_MS);
    return () => {
      clearInterval(this.timer);
      this.timer = undefined;
    };
  }

  /** What the hub says about this host (`GET /api/app/config`): its
   * block domain. A hub from before it (404), or one that can't say, leaves
   * none. */
  async config() {
    try {
      const res = await this.send("/api/app/config", { credentials: "same-origin" });
      const v = res.ok ? ((await res.json().catch(() => null)) as { block_domain?: unknown } | null) : null;
      const d = v?.block_domain;
      this.blockDomain = typeof d === "string" && /^[a-z0-9.-]+(:\d{1,5})?$/i.test(d) ? d : null;
    } catch {
      this.blockDomain = null;
    }
  }

  /** Read the machine list again. */
  async refresh() {
    const me = this.account();
    let res: Response;
    try {
      res = await this.send("/api/directory", { credentials: "same-origin" });
    } catch {
      return this.lost();
    }
    if (res.status === 404) return this.unreachable();
    if (res.status === 401) {
      this.phase = "signed-out";
      this.daemons = [];
      return this.emit();
    }
    const dir = res.ok ? ((await res.json().catch(() => null)) as { daemons?: ForeignEntry[] } | null) : null;
    if (!dir || !Array.isArray(dir.daemons)) return this.lost();
    // A machine control gives no Noise key for (a control from before
    // #994) can't be reached with a token from here, and is left out.
    this.daemons = dir.daemons.flatMap((d) => {
      const entry = tokenEntry(d, [], me?.account ?? "", this.teams);
      return entry ? [entry] : [];
    });
    this.login = me?.login ?? "";
    this.stale = false;
    this.reachable = true;
    this.phase = "ready";
    this.emit();
  }

  private lost() {
    this.stale = true;
    this.daemons = this.daemons.map((d) => ({ ...d, online: false }));
    if (this.phase === "loading") this.phase = "error";
    this.emit();
  }

  /** How a Client reaches daemon `id`: the relay on this page's origin,
   * with a machine token, and no device key. Not a machine's own address:
   * this page's CSP lets it connect to its own origin only. */
  target(id: string): E2ETarget | undefined {
    const d = this.daemons.find((x) => x.id === id);
    if (!d) return undefined;
    const ws = this.origin.replace(/^http/, "ws");
    const hosted = d.hosted ? { project: d.hosted.project, workspace: d.hosted.workspace } : undefined;
    return {
      daemon: { id: d.id, noise: d.cert.noise },
      direct: [],
      relay: `${ws}/api/relay/c/${d.id}`,
      mux: `${ws}/api/relay/m`,
      tokens: this.tokens,
      ...(this.blockDomain ? { blockDomain: this.blockDomain } : {}),
      hosted,
    };
  }

  /** Today's link to control's page for `hash` (`pane=<machine>.<pane>`,
   * `machine=<id>`, `swarm`): where a machine opens while it isn't
   * `reachable` from here. */
  fallback(hash: string): string {
    return `${this.control}/#${hash}`;
  }

  /** The account's prefs for its pages (shared/seen.ts's backend). */
  async prefs(): Promise<Record<string, unknown>> {
    return this.json(await this.send("/api/me/prefs", { credentials: "same-origin" }));
  }

  /** Set some (`null` takes one back); the rest stay. */
  async setPrefs(changes: Record<string, unknown>): Promise<Record<string, unknown>> {
    return this.json(
      await this.send("/api/me/prefs", { method: "POST", credentials: "same-origin", headers: { "Content-Type": "application/json" }, body: JSON.stringify(changes) }),
    );
  }

  private async json(res: Response): Promise<Record<string, unknown>> {
    const j = (await res.json().catch(() => ({}))) as Record<string, unknown>;
    if (!res.ok) throw new Error(typeof j.error === "string" ? j.error : `HTTP ${res.status}`);
    return j;
  }
}
