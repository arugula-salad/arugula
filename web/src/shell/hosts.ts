// The hosts this page can switch between (M4a): the daemon it was loaded
// from (the home daemon) and the others on that daemon's list. Each host
// owns its own layout, and the client connects to whichever is shown
// directly; the home daemon never relays. The last list (and which host was
// shown) is kept in localStorage, so known hosts stay reachable while the
// home daemon is down.
import { opPath } from "./ops";
import type { HostInfo } from "../shared/proto";

export interface Host {
  name: string;
  urls: string[];
  /** A daemon in Arugula control's directory: its device id. */
  id?: string;
  /** How it's reached: `tailnet`, straight to its URLs; `dial_out` (M4c),
   * it dials the home daemon and is reached through it at `/h/<name>/…`
   * (on this page's own origin); `control`, through control's directory;
   * `ssh` (M51), reached by a terminal's own ssh, never from a page. */
  transport: "tailnet" | "dial_out" | "control" | "ssh";
  /** An ssh host's destination (`user@box`). */
  ssh?: string;
  added_ms: number;
  last_seen_ms: number | null;
  /** A control host's state in the directory: online, offline. */
  status?: string;
}

export interface HostList {
  this: string;
  hosts: Host[];
}

const LIST_KEY = "arugula.hosts";
const SHOWN_KEY = "arugula.host";
const CONTROL_SHOWN_KEY = "arugula.control.host";

function load<T>(key: string): T | null {
  try {
    const v = localStorage.getItem(key);
    return v ? (JSON.parse(v) as T) : null;
  } catch {
    return null;
  }
}

function save(key: string, v: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(v));
  } catch {
    // Private mode or storage off: just not remembered.
  }
}

export class HostDirectory {
  /** The last list we got (or the cached one until then). */
  list: HostList | null = load<HostList>(LIST_KEY);
  /** The list is the cached one: the home daemon didn't answer. */
  stale = true;
  /** The host shown; `null` is the home daemon (this page's own). */
  shown: string | null = load<string>(SHOWN_KEY);
  /** Provider hosts whose tailnet URL answered: used instead of the tunnel. */
  /** The page is Arugula control's (M17): the list comes from control,
   * and there's no home daemon. */
  control = false;
  /** The control this page's daemon joined (`/api/host`'s `control`), if
   * any: the host menu links to its page for the account's other machines
   * (M49), rather than listing them here. */
  joined: string | null = null;
  /** Control mode: how the shown host is reached, for the host chip. */
  path: "direct" | "relayed" | null = null;

  setPath(p: "direct" | "relayed" | null) {
    if (p === this.path) return;
    this.path = p;
    this.emit();
  }
  private listeners = new Set<() => void>();

  constructor() {
    // A link to a machine (`?machine=geek`) shows it, over the one this
    // browser showed last.
    const asked = linkedMachine();
    if (asked !== null && this.find(asked)) this.shown = asked;
    if (this.shown !== null && !this.find(this.shown)) this.shown = null;
  }

  /** Keep the address on the machine shown (`?machine=NAME`), so the view
   * can be linked to and survives a reload; the home daemon's own page
   * needs none. A route in the hash (#chat, #swarm…) stays. */
  private syncUrl() {
    const name = this.control || this.shown !== null ? this.current : "";
    const params = new URLSearchParams(location.search);
    if ((params.get("machine") ?? "") === name) return;
    if (name) params.set("machine", name);
    else params.delete("machine");
    const q = params.toString();
    history.replaceState(history.state, "", `${location.pathname}${q ? `?${q}` : ""}${location.hash}`);
  }

  subscribe(fn: () => void): () => void {
    this.listeners.add(fn);
    return () => this.listeners.delete(fn);
  }

  private emit() {
    for (const fn of this.listeners) fn();
  }

  /** The home daemon's name, once known. */
  get home(): string | null {
    return this.control ? null : (this.list?.this ?? null);
  }

  /** Every host, the home daemon first. */
  /** The hosts this page can switch to. ssh hosts aren't among them: only
   * a terminal reaches those (`sshOnly`). */
  get names(): string[] {
    const reachable = (this.list?.hosts ?? []).filter((h) => h.transport !== "ssh").map((h) => h.name);
    if (this.control) return reachable;
    return this.list ? [this.list.this, ...reachable] : [];
  }

  /** ssh hosts (M51): listed so they aren't a surprise, reached from a
   * terminal with `arugula --host NAME …`. */
  get sshOnly(): Host[] {
    return (this.list?.hosts ?? []).filter((h) => h.transport === "ssh");
  }

  /** Control mode: the daemons this browser checked, from control. */
  setControl(hosts: Host[], stale: boolean) {
    const first = !this.control;
    // The shown machine by its id, to follow it if it was renamed (#500).
    const shownId = this.shown !== null ? this.find(this.shown)?.id : undefined;
    this.control = true;
    this.list = { this: "", hosts };
    const now = shownId ? hosts.find((h) => h.id === shownId) : undefined;
    if (now && now.name !== this.shown) {
      this.shown = now.name;
      save(CONTROL_SHOWN_KEY, now.name);
      this.syncUrl();
    }
    this.stale = stale;
    if (first) {
      const asked = linkedMachine();
      this.shown = asked !== null && this.find(asked) ? asked : load<string>(CONTROL_SHOWN_KEY);
    }
    if (this.shown !== null && !this.find(this.shown)) this.shown = null;
    if (hosts.length) this.syncUrl();
    this.emit();
  }

  find(name: string): Host | undefined {
    return this.list?.hosts.find((h) => h.name === name);
  }

  /** What the client prefixes its URLs with: "" for this page's daemon. */
  base(name: string | null = this.shown): string {
    if (this.control) {
      const h = this.find(name ?? this.names[0] ?? "");
      return h?.id ? `e2e:${h.id}` : "";
    }
    if (name === null || name === this.home) return "";
    const h = this.find(name);
    // The one place a host's URL is chosen.
    if (h?.transport === "dial_out") return `/h/${encodeURIComponent(h.name)}`;
    return h?.urls[0] ?? "";
  }

  /** The shown host's name ("" until the home daemon's is known). */
  get current(): string {
    return this.shown ?? this.home ?? this.names[0] ?? "";
  }

  select(name: string) {
    const shown = name === this.home ? null : name;
    if (shown === this.shown) return;
    this.shown = shown;
    save(this.control ? CONTROL_SHOWN_KEY : SHOWN_KEY, shown);
    this.syncUrl();
    this.emit();
  }

  /** Fetch the list from the home daemon; on failure keep the cached one. */
  async refresh() {
    if (this.control) return;
    try {
      const res = await fetch(opPath("hosts.list"));
      if (!res.ok) throw new Error(String(res.status));
      this.list = (await res.json()) as HostList;
      this.stale = false;
      save(LIST_KEY, this.list);
      // A host that left the list can't stay shown.
      if (this.shown !== null && !this.find(this.shown)) {
        this.shown = null;
        save(SHOWN_KEY, null);
      }
      // A link to a machine the cached list didn't have yet.
      const asked = linkedMachine();
      if (asked !== null && asked !== this.shown && this.find(asked)) this.shown = asked;
      this.syncUrl();
    } catch {
      this.stale = true;
    }
    try {
      const res = await fetch(opPath("host.get"));
      if (res.ok) this.joined = ((await res.json()) as Partial<HostInfo>).control ?? null;
    } catch {
      // Keep what we knew.
    }
    this.emit();
  }
}

/** The machine the address names (`?machine=NAME`), if any. */
function linkedMachine(): string | null {
  try {
    return new URLSearchParams(location.search).get("machine");
  } catch {
    return null;
  }
}

export const directory = new HostDirectory();
