// The sessions this account pinned, as the platform keeps them
// (session-pins.ts says where each list puts them). Read once a list asks
// for them, written whole on each change (the last write wins), and read
// again when chat's live stream says another of the account's pages
// changed them (`pins`). A page the platform doesn't answer (a daemon's own
// page, signed out, a platform from before pins) has none and offers no
// Pin: there is no second store for a machine with no account.

import type { SessionPin, SessionPins } from "./proto";
import { platformFetch } from "./maintenance";

export const PINS_PATH = "/api/sessions/pins";

async function call(method: "GET" | "PUT", body?: SessionPins): Promise<SessionPins | null> {
  try {
    const res = await platformFetch(PINS_PATH, {
      method,
      headers: body ? { "Content-Type": "application/json" } : {},
      body: body ? JSON.stringify(body) : undefined,
      credentials: "same-origin",
    });
    if (!res.ok) return null;
    const j = (await res.json()) as SessionPins;
    return Array.isArray(j?.pins) ? j : null;
  } catch {
    return null;
  }
}

class PinsStore {
  /** Oldest first. */
  pins: SessionPin[] = [];
  /** Whether the platform keeps pins for this page: null until it said. */
  on: boolean | null = null;
  private asked = false;
  /** Edits sent and not yet answered: what is read meanwhile is older. */
  private pending = 0;
  private listeners = new Set<() => void>();

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private emit() {
    for (const fn of this.listeners) fn();
  }

  /** Read them, the first time a list that can show them asks. */
  want() {
    if (this.asked) return;
    this.asked = true;
    void this.load();
  }

  /** Read them again: the live stream said they changed, or came back. */
  heard() {
    if (this.asked) void this.load();
  }

  private async load() {
    const got = await call("GET");
    if (this.pending) return;
    if (!got) {
      // Kept as they were if the platform answered before: a dropped read
      // isn't an empty list.
      if (this.on === null) {
        this.on = false;
        this.emit();
      }
      return;
    }
    this.on = true;
    this.pins = got.pins;
    this.emit();
  }

  /** Change them: shown at once, kept on the platform, and told to the
   * account's other pages. If the platform refuses, they're read again so
   * the page shows what is kept. */
  async edit(change: (pins: SessionPin[]) => SessionPin[]) {
    if (!this.on) return;
    const next = change(this.pins);
    if (next === this.pins) return;
    this.pins = next;
    this.emit();
    this.pending++;
    const kept = await call("PUT", { pins: next });
    this.pending--;
    if (!kept) return void this.load();
    if (!this.pending) {
      this.pins = kept.pins;
      this.emit();
    }
  }
}

export const sessionPins = new PinsStore();
