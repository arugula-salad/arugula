// Who's around (#110): this page's heartbeat and what control says of the
// people who share a team with the reader. The page says it's open every
// BEAT and whether it was used lately; control keeps only when it last
// heard, and answers with everyone's state, so one request is both the
// beat and the poll. Nothing here knows chat's rooms: the store starts it
// when chat is on and stops it when it isn't.

import type { ChatBeat, ChatPresence, PresenceMode, PresenceState } from "../proto";
import { platformFetch } from "../maintenance";
import type { MenuItem } from "../ui/menu";

const BEAT = 15_000;
/** A page not used for this long says it's idle (control: away). */
const IDLE = 10 * 60_000;
/** Pointer moves are cheap but not worth a clock read each. */
const INPUTS = ["pointerdown", "pointermove", "keydown", "wheel", "touchstart"] as const;

class PresenceStore {
  private people = new Map<string, PresenceState>();
  private online = new Set<string>();
  private asked = new Set<string>();
  private listeners = new Set<() => void>();
  private timer: number | undefined;
  private last = Date.now();
  private running = false;
  /** The reader's own choice (auto until control says). */
  mode: PresenceMode = "auto";

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => void this.listeners.delete(fn);
  }

  /** An account's state; null for someone control doesn't show (not on a
   * team with the reader, or not heard of yet). */
  of(account: string): PresenceState | null {
    return this.people.get(account) ?? null;
  }

  /** Whether `machine` (an agent's) is connected. Asking for one adds it
   * to what the next beat wants. */
  machine(machine: string): boolean {
    if (!this.asked.has(machine)) {
      this.asked.add(machine);
      if (this.running) this.soon();
    }
    return this.online.has(machine);
  }

  /** Whether it's running: while chat is on. */
  get on(): boolean {
    return this.running;
  }

  start() {
    if (this.running) return;
    this.running = true;
    this.last = Date.now();
    for (const e of INPUTS) addEventListener(e, this.input, { passive: true, capture: true });
    addEventListener("visibilitychange", this.visible);
    addEventListener("pagehide", this.leave);
    void this.beat();
  }

  stop() {
    if (!this.running) return;
    this.running = false;
    clearTimeout(this.timer);
    for (const e of INPUTS) removeEventListener(e, this.input, { capture: true });
    removeEventListener("visibilitychange", this.visible);
    removeEventListener("pagehide", this.leave);
    this.people.clear();
    this.online.clear();
    this.emit();
  }

  /** Appear offline, away or as the page says (`auto`). */
  async setMode(mode: PresenceMode) {
    this.mode = mode;
    this.emit();
    await this.beat(mode);
  }

  private emit() {
    for (const fn of [...this.listeners]) fn();
  }

  private input = () => {
    const was = Date.now() - this.last > IDLE;
    this.last = Date.now();
    // Back after being idle: say so now, not at the next beat.
    if (was) this.soon();
  };

  private visible = () => {
    if (document.visibilityState === "visible") this.soon();
  };

  /** A closing page is offline for its teammates now, not when the beat
   * goes stale. */
  private leave = () => {
    const body: ChatBeat = { idle: true, gone: true, mode: null, machines: [] };
    navigator.sendBeacon?.("/api/chat/presence", new Blob([JSON.stringify(body)], { type: "application/json" }));
  };

  private soon() {
    clearTimeout(this.timer);
    this.timer = window.setTimeout(() => void this.beat(), 200);
  }

  private async beat(mode?: PresenceMode) {
    clearTimeout(this.timer);
    try {
      const body: ChatBeat = {
        idle: Date.now() - this.last > IDLE,
        gone: false,
        mode: mode ?? null,
        machines: [...this.asked].slice(0, 64),
      };
      const res = await platformFetch("/api/chat/presence", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
        credentials: "same-origin",
      });
      if (res.ok) {
        const p = (await res.json()) as ChatPresence;
        this.people = new Map(p.people.map((x) => [x.account, x.state]));
        this.online = new Set(p.machines_online);
        this.mode = p.mode;
        this.emit();
      }
    } catch {
      // Control didn't answer: as it was, and beat again.
    }
    if (this.running) this.timer = window.setTimeout(() => void this.beat(), BEAT);
  }
}

export const presence = new PresenceStore();

/** The account menu's lines for the reader's own presence, when chat is
 * on: automatic, away by hand, or always appearing offline. */
export function presenceItems(): MenuItem[] {
  if (!presence.on) return [];
  const item = (label: string, mode: PresenceMode): MenuItem => ({ label, checked: presence.mode === mode, run: () => void presence.setMode(mode) });
  return [{ header: "Your presence" }, item("Automatic", "auto"), item("Away", "away"), item("Appear offline", "offline"), "separator"];
}
