// The live presence store (Arugula #343): a page's one connection to its
// teams' rooms on arugula.stream, which everything live draws from: chat's
// avatars (#344), the shell's, and the People block (#346).
//
// While the switch (./switch, #341) says Watching or Live, it's in the
// room of every team the page says the reader is in (`team:<id>`, with a
// token control signs, fetched again on every reconnect). A People block
// can hold one team's room open whatever the switch says (`hold`). Live is
// this window's camera, sent to one room: the team the page says the
// reader is working in, else the first. It stops while the window is
// hidden. Each room says its state (connecting, on, refused, unreachable),
// so nothing that draws from it goes blank without saying why.
//
// The SDK is arugula.stream's own (`<relay>/sdk/v0/presence.js`, loaded
// once): the page passes it in (./live), with the token fetch and the
// switch, so a test can drive this with fakes.

export type LiveState = "off" | "watching" | "live";
export type RoomState = "connecting" | "on" | "refused" | "unreachable";

/** One person in a room, as arugula.stream says. */
export interface SdkMember {
  sub: string;
  name: string;
  mode: "avatar" | "outline" | "none";
  away: boolean;
}

/** What this uses of a connected room (the SDK's `Room`). */
export interface SdkRoom {
  readonly members: SdkMember[];
  on(event: "members" | "live", fn: (...args: never[]) => void): () => void;
  readonly camera: { start(o?: { mode?: "avatar" | "outline" }): Promise<void>; stop(): void; readonly on: boolean };
  avatar(el: Element, sub: string, o?: { size?: "head" | "bust"; draw?: "live" | "character" }): () => void;
  leave(): void;
}

export interface Sdk {
  connect(o: { room: string; relay: string; token: () => Promise<string> }): Promise<SdkRoom>;
}

/** Control's answer for a room: its token, and the relay it's on. */
export type Token = { token: string; relay: string };

/** A refusal from control (or the relay): it says why, and trying again
 * the same way won't help. */
export class Refused extends Error {}

export interface Deps {
  /** The switch's state here, or null (flag or chat off). */
  state(): LiveState | null;
  /** Called when the switch changes. */
  subscribe(fn: () => void): () => void;
  /** A room token for `team:<id>` (throws `Refused`, or anything else for
   * control not answering). */
  token(team: string): Promise<Token>;
  /** arugula.stream's SDK, from the relay's site. */
  sdk(relay: string): Promise<Sdk>;
}

interface Entry {
  room: SdkRoom | null;
  state: RoomState;
  why: string;
  stop(): void;
}

/** A person to draw live: the room they're in and how. */
export interface LiveFace {
  room: SdkRoom;
  member: SdkMember;
}

export class LiveStore {
  private teams: string[] = [];
  private working: string | null = null;
  private held = new Map<string, number>();
  private rooms = new Map<string, Entry>();
  private listeners = new Set<() => void>();
  private hidden = false;
  private camera: string | null = null;
  private deps: Deps;

  constructor(deps: Deps) {
    this.deps = deps;
    deps.subscribe(() => this.sync());
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => void this.listeners.delete(fn);
  }

  /** The teams the reader is in, as this page knows them. */
  setTeams(teams: string[]) {
    const next = [...new Set(teams)].sort();
    if (next.join() === this.teams.join()) return;
    this.teams = next;
    this.sync();
  }

  /** The team the reader is working in here: Live's camera goes to its room. */
  setWorking(team: string | null) {
    if (team === this.working) return;
    this.working = team;
    this.syncCamera();
  }

  /** Whether the window is hidden: Live's camera stops while it is. */
  setHidden(hidden: boolean) {
    if (hidden === this.hidden) return;
    this.hidden = hidden;
    this.syncCamera();
  }

  /** Keep `team`'s room open whatever the switch says (a People block shown,
   * #346), until the returned function is called. */
  hold(team: string): () => void {
    this.held.set(team, (this.held.get(team) ?? 0) + 1);
    this.sync();
    let done = false;
    return () => {
      if (done) return;
      done = true;
      const n = (this.held.get(team) ?? 1) - 1;
      if (n > 0) this.held.set(team, n);
      else this.held.delete(team);
      this.sync();
    };
  }

  /** `team`'s room, and its state and why (for a refusal), or null when
   * this page isn't in it. */
  room(team: string): { room: SdkRoom | null; state: RoomState; why: string } | null {
    const e = this.rooms.get(team);
    return e ? { room: e.room, state: e.state, why: e.why } : null;
  }

  /** Leave `team`'s room and join it again (a refused or unreachable one's
   * Try again). */
  retry(team: string) {
    this.rooms.get(team)?.stop();
    this.rooms.delete(team);
    this.sync();
  }

  /** `account` live in a room this page is in: their camera on (a
   * character or an outline) and not away; else null. */
  live(account: string): LiveFace | null {
    for (const e of this.rooms.values()) {
      const m = e.room?.members.find((x) => x.sub === account);
      if (e.room && m && m.mode !== "none" && !m.away) return { room: e.room, member: m };
    }
    return null;
  }

  /** The rooms this page is in, by team, for a test or a status line. */
  states(): Record<string, RoomState> {
    return Object.fromEntries([...this.rooms].map(([t, e]) => [t, e.state]));
  }

  private emit() {
    for (const fn of [...this.listeners]) fn();
  }

  private wanted(): Set<string> {
    const s = this.deps.state();
    const all = s === "watching" || s === "live" ? this.teams : [];
    return new Set([...all, ...this.held.keys()]);
  }

  private sync() {
    const want = this.wanted();
    for (const [team, e] of [...this.rooms]) {
      if (!want.has(team)) {
        if (this.camera === team) this.camera = null;
        e.stop();
        this.rooms.delete(team);
      }
    }
    for (const team of want) if (!this.rooms.has(team)) void this.join(team);
    this.syncCamera();
    this.emit();
  }

  private async join(team: string) {
    const e: Entry = { room: null, state: "connecting", why: "", stop: () => {} };
    let gone = false;
    e.stop = () => void (gone = true);
    this.rooms.set(team, e);
    this.emit();
    try {
      const first = await this.deps.token(team);
      const sdk = await this.deps.sdk(first.relay);
      // The first token is the one just fetched; every reconnect asks again.
      let fresh: string | null = first.token;
      const token = async () => {
        const t = fresh;
        fresh = null;
        return t ?? (await this.deps.token(team)).token;
      };
      const room = await sdk.connect({ room: `team:${team}`, relay: first.relay, token });
      if (gone || this.rooms.get(team) !== e) return room.leave();
      e.room = room;
      e.state = "on";
      const offs = [
        room.on("members", () => this.emit()),
        room.on("live", ((s: string) => {
          e.state = s === "on" ? "on" : s === "connecting" ? "connecting" : s === "refused" ? "refused" : "unreachable";
          if (s === "refused") e.why = "arugula.stream refused this room";
          this.emit();
        }) as (...args: never[]) => void),
      ];
      e.stop = () => {
        for (const off of offs) off();
        if (room.camera.on) room.camera.stop();
        room.leave();
      };
      this.syncCamera();
    } catch (err) {
      if (gone || this.rooms.get(team) !== e) return;
      e.state = err instanceof Refused ? "refused" : "unreachable";
      e.why = err instanceof Error ? err.message : String(err);
    }
    this.emit();
  }

  /** Live: the camera on in one room (the working team's, else the first
   * joined), off everywhere else, and off while the window is hidden. */
  private syncCamera() {
    const live = this.deps.state() === "live" && !this.hidden;
    const on = [...this.rooms].filter(([, e]) => e.room);
    const target = live ? (on.find(([t]) => t === this.working) ?? on[0])?.[0] ?? null : null;
    for (const [team, e] of on) {
      const cam = e.room!.camera;
      if (team === target && !cam.on) void cam.start({ mode: "avatar" }).catch(() => {});
      if (team !== target && cam.on) cam.stop();
    }
    this.camera = target;
  }
}
