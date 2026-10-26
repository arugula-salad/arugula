// The activity drawer (M112b, #1027): what happened in the selected session,
// below its tab. It is a drawer, never a pane: the tab's layout doesn't know
// it's there, and the panes above it re-measure through TabArea's own
// ResizeObserver when it opens, closes or is dragged taller.
//
// Commands, answers, agent turns and actions come from `GET /api/history?
// session=` (M112a), whose ids are stable, so a poll or Refresh merges by id
// and never shows one twice. System (exit, opened, closed, machine) comes
// from `GET /api/events?session=` the same way: the daemon keeps those
// (#1035), with the ids `follow=1` streams them with, so the stream (where
// a plain fetch reaches the daemon) only makes them show sooner. A daemon
// from before #1035 keeps none: its System is what the stream brought.

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import type { Client } from "../../client";
import type { PaneId, SessionId } from "../../../shared/proto";
import { useSubscribe } from "../../../shared/ui/hooks";
import { openMenu } from "../../../shared/ui/menu";
import { closePlace, currentPlace } from "../places";
import { MoreIcon } from "./icons";

// ---- what this device remembers

export type ActivityFilter = "all" | "agents" | "commands" | "system";

interface Prefs {
  open: boolean;
  height: number;
  live: boolean;
  filter: ActivityFilter;
}

const KEY = "arugula.workspace.activity";
const MIN_H = 96;
const DEFAULT: Prefs = { open: false, height: 220, live: true, filter: "all" };
const FILTERS: ActivityFilter[] = ["all", "agents", "commands", "system"];

function load(): Prefs {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "null") as Partial<Prefs> | null;
    if (!v) return DEFAULT;
    return {
      open: v.open === true,
      height: typeof v.height === "number" ? Math.max(MIN_H, Math.round(v.height)) : DEFAULT.height,
      live: v.live !== false,
      filter: FILTERS.includes(v.filter as ActivityFilter) ? (v.filter as ActivityFilter) : "all",
    };
  } catch {
    return DEFAULT;
  }
}

let prefs = load();
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

function setPrefs(p: Partial<Prefs>, keep = true) {
  prefs = { ...prefs, ...p };
  if (keep) {
    try {
      localStorage.setItem(KEY, JSON.stringify(prefs));
    } catch {
      // Private mode: it lasts for this page.
    }
  }
  changed();
}

function usePrefs(): Prefs {
  useSubscribe((fn) => {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  });
  return prefs;
}

/** Open or collapse the drawer (the footer's toggle, and its own menu). */
export function setActivityOpen(open: boolean) {
  setPrefs({ open });
}

/** Daemons that answered without ids: too old for the drawer. It hides. */
const unavailable = new WeakSet<Client>();

// ---- the feed

type Kind = "command" | "answer" | "agent" | "action" | "system";

interface Item {
  id: string;
  /** When this page first saw it: what Clear view and a paused view cut at. */
  seq: number;
  pane: PaneId | null;
  kind: Kind;
  text: string;
  at: number;
  ended: number | null;
  exit: number | null;
  /** Its pane is still open (history keeps a closed pane's a while). */
  open: boolean;
  by?: string;
}

/** `GET /api/history`'s entry (crates/proto/src/api.rs; not in proto.gen). */
interface HistoryEntry {
  id?: string;
  pane: PaneId;
  open: boolean;
  text: string | null;
  exit: number | null;
  started_ms: number;
  ended_ms: number | null;
  kind?: Kind;
  by?: string;
}

const SYSTEM_TYPES = "exit,opened,closed,machine";
/** How far back System reads, in seconds: as far as history's commands go
 * in practice, without reading a week of opens and closes each poll. */
const SYSTEM_SINCE = 24 * 3600;

/** `GET /api/events`'s line, for the types System asks for. */
interface EventLine {
  id?: string;
  at_ms: number;
  pane?: PaneId;
  type: string;
  code?: number | null;
  machine_gone?: boolean;
  machine?: number;
  state?: string;
}

type Status = "loading" | "ready" | "error" | "denied" | "unavailable";

const credentials = (base: string): RequestCredentials => (/^https?:/.test(base) ? "include" : "same-origin");

class Feed {
  readonly items = new Map<string, Item>();
  seq = 0;
  status: Status = "loading";
  error: string | null = null;
  /** Whether System streams here as well as being read: a stream needs a
   * plain fetch, which a page reaching its daemon through control's
   * channel doesn't have (it reads System with each poll instead). */
  readonly live: boolean;
  private readonly fns = new Set<() => void>();
  private stopped = false;
  private abort: AbortController | null = null;
  private retry: number | undefined;
  private busy: Promise<void> | null = null;

  constructor(
    readonly client: Client,
    readonly session: SessionId,
  ) {
    this.live = !client.e2e;
  }

  subscribe(fn: () => void) {
    this.fns.add(fn);
    return () => void this.fns.delete(fn);
  }

  private emit() {
    this.fns.forEach((fn) => fn());
  }

  private put(it: Omit<Item, "seq">) {
    const was = this.items.get(it.id);
    this.items.set(it.id, { ...it, seq: was?.seq ?? ++this.seq });
  }

  /** The session's history again, merged by id. One at a time. */
  refresh(): Promise<void> {
    this.busy ??= this.fetch().finally(() => (this.busy = null));
    return this.busy;
  }

  private async fetch() {
    try {
      const [res] = await Promise.all([
        this.client.request("GET", `/api/history?session=${this.session}&limit=200`),
        this.fetchSystem(),
      ]);
      if (this.stopped) return;
      if (res.status === 403 || (res.status === 404 && !this.client.ownsMachine())) {
        this.status = "denied";
      } else if (!res.ok) {
        const v = await res.json<{ error?: string }>().catch(() => null);
        this.status = "error";
        this.error = v?.error ?? `couldn't read the history (${res.status})`;
      } else {
        const all = await res.json<HistoryEntry[]>();
        // A daemon from before M112 ignores `session=` and gives no ids:
        // its entries would be every session's, and couldn't be merged.
        if (all.some((e) => !e.id)) {
          this.status = "unavailable";
          unavailable.add(this.client);
          changed();
        } else {
          this.status = "ready";
          this.error = null;
          for (const e of all) {
            this.put({
              id: e.id!,
              pane: e.pane,
              kind: e.kind ?? "command",
              text: e.text ?? "",
              at: e.started_ms,
              ended: e.ended_ms,
              exit: e.exit,
              open: e.open,
              by: e.by,
            });
          }
        }
      }
    } catch (e) {
      if (this.stopped) return;
      this.status = "error";
      this.error = e instanceof Error ? e.message : String(e);
    }
    this.emit();
  }

  /** System's kept events (#1035). Not reading them is no error: History
   * says how the session is, and the stream may still bring them. */
  private async fetchSystem() {
    try {
      const path = `/api/events?session=${this.session}&type=${SYSTEM_TYPES}&since=${SYSTEM_SINCE}`;
      const res = await this.client.request("GET", path);
      if (!res.ok || !res.text || this.stopped) return;
      for (const line of (await res.text()).split("\n")) {
        if (line.trim()) this.event(line);
      }
    } catch {
      // Left to the next poll.
    }
  }

  /** System's events, as they happen, until stopped. */
  stream() {
    if (!this.live || this.stopped) return;
    const ctrl = (this.abort = new AbortController());
    const base = this.client.base;
    const url = `${base}/api/events?session=${this.session}&follow=1&type=${SYSTEM_TYPES}`;
    const again = () => {
      if (!this.stopped) this.retry = window.setTimeout(() => this.stream(), 5000);
    };
    void (async () => {
      try {
        const res = await fetch(url, {
          credentials: credentials(base),
          signal: ctrl.signal,
        });
        if (!res.ok || !res.body) return again();
        const reader = res.body.getReader();
        const dec = new TextDecoder();
        let buf = "";
        for (;;) {
          const { done, value } = await reader.read();
          if (done) break;
          buf += dec.decode(value, { stream: true });
          let nl;
          while ((nl = buf.indexOf("\n")) >= 0) {
            const line = buf.slice(0, nl).trim();
            buf = buf.slice(nl + 1);
            if (line) {
              this.event(line);
              this.emit();
            }
          }
        }
        again();
      } catch {
        again();
      }
    })();
  }

  private event(line: string) {
    let e: EventLine;
    try {
      e = JSON.parse(line) as EventLine;
    } catch {
      return;
    }
    const text = systemText(e);
    if (!text) return;
    // The stream gives an event the id a read of it does (#1035), so the
    // two merge; a line without one (no daemon gives none now) falls back.
    const id = `sys:${e.id || `${e.type}:${e.pane ?? e.machine}:${e.at_ms}`}`;
    this.put({
      id,
      pane: e.pane ?? null,
      kind: "system",
      text,
      at: e.at_ms,
      ended: null,
      exit: e.type === "exit" ? (e.code ?? null) : null,
      open: true,
    });
  }

  stop() {
    this.stopped = true;
    this.abort?.abort();
    clearTimeout(this.retry);
  }
}

function systemText(e: EventLine): string | null {
  switch (e.type) {
    case "opened":
      return "Pane opened";
    case "closed":
      return "Pane closed";
    case "exit":
      if (e.machine_gone) return "Its machine went away";
      return e.code == null ? "Exited" : `Exited with ${e.code}`;
    case "machine":
      return `Machine ${e.machine ?? "?"} ${e.state === "gone" ? "is gone" : (e.state ?? "changed")}`;
    default:
      return null;
  }
}

const inFilter = (f: ActivityFilter, k: Kind) =>
  f === "all" || (f === "agents" ? k === "agent" || k === "answer" : f === "commands" ? k === "command" : k === "system");

const LABEL: Record<ActivityFilter, string> = {
  all: "All",
  agents: "Agents",
  commands: "Commands",
  system: "System",
};
const KIND: Record<Kind, string> = {
  command: "cmd",
  answer: "answer",
  agent: "agent",
  action: "action",
  system: "system",
};

const clock = (ms: number) =>
  new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hour12: false,
  });

/** A pane by what it runs, not its id (UI audit #171): the id is in the
 * row's tooltip. */
function paneName(client: Client, pane: PaneId): string {
  const t = client.title(pane) || client.info(pane)?.command || client.cwd(pane)?.split("/").filter(Boolean).pop() || "";
  return t || `pane ${pane}`;
}

/** Who did it, as one name (UI audit #171): history says a person by the
 * name they came with, which is a login through control and a tailnet
 * login (`jake@github`, `jake@example.com`) over the tailnet. The part
 * before the @ is that login; the whole of it is the tooltip. */
export function byName(by: string): string {
  const at = by.indexOf("@");
  return at > 0 ? by.slice(0, at) : by;
}

// ---- the drawer

/** The selected session's activity, below its tab: collapsed to its bar,
 * or open at the height this device keeps. */
export function ActivityDrawer({ client, session }: { client: Client; session: SessionId }) {
  const p = usePrefs();
  const feed = useMemo(() => new Feed(client, session), [client, session]);
  const [, tick] = useState(0);
  useEffect(() => feed.subscribe(() => tick((t) => t + 1)), [feed]);
  // Where this page's view stops: Clear view hides up to `cleared`; a
  // paused (not Live) view shows up to `frozen`.
  const [cleared, setCleared] = useState(0);
  const [frozen, setFrozen] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);

  useEffect(() => {
    setCleared(0);
    void feed.refresh().then(() => setFrozen(feed.seq));
    feed.stream();
    return () => feed.stop();
  }, [feed]);

  // Collecting goes on while it's open, Live or not; Live decides whether
  // the view follows.
  useEffect(() => {
    if (!p.open) return;
    const t = window.setInterval(() => {
      if (document.visibilityState === "visible") void feed.refresh();
    }, 4000);
    return () => clearInterval(t);
  }, [feed, p.open]);

  const all = [...feed.items.values()].filter((it) => it.seq > cleared).sort((a, b) => a.at - b.at || a.seq - b.seq);
  const upTo = p.live ? Infinity : frozen;
  const kept = all.filter((it) => it.seq <= upTo);
  const waiting = all.length - kept.length;
  const filter = p.filter;
  const shown = kept.filter((it) => inFilter(filter, it.kind));

  // Live keeps the newest in view, unless you've scrolled up to read.
  useLayoutEffect(() => {
    const el = list.current;
    if (el && p.live && atBottom.current) el.scrollTop = el.scrollHeight;
  });

  if (unavailable.has(client)) return null;

  const refresh = () => void feed.refresh().then(() => setFrozen(feed.seq));
  const live = () => {
    if (p.live) setFrozen(feed.seq);
    atBottom.current = true;
    setPrefs({ live: !p.live });
  };
  const more = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openMenu(
      {
        clientX: r.right - 180,
        clientY: r.top - 4,
        preventDefault: () => e.preventDefault(),
      },
      [
        { label: "Clear view", run: () => setCleared(feed.seq) },
        p.open ? { label: "Collapse", run: () => setActivityOpen(false) } : { label: "Expand", run: () => setActivityOpen(true) },
      ],
    );
  };
  const reveal = (it: Item) => {
    if (it.pane === null || !client.info(it.pane)) return;
    const at = currentPlace();
    if (at !== "panes") closePlace(at);
    client.setActive(it.pane);
    client.viewOf(it.pane)?.focus();
  };

  return (
    <section
      class={p.open ? "ws-activity open" : "ws-activity"}
      data-activity-drawer
      data-activity-status={feed.status}
      aria-label="Activity"
      style={p.open ? { height: `${p.height}px` } : undefined}
    >
      {p.open && <Resize />}
      <header class="ws-activity-bar">
        <button
          class="ws-activity-title"
          aria-expanded={p.open}
          title={p.open ? "Collapse the activity drawer" : "Open the activity drawer"}
          onClick={() => setActivityOpen(!p.open)}
        >
          <Chevron open={p.open} />
          Activity
        </button>
        <div class="ws-activity-filters" role="group" aria-label="Show">
          {FILTERS.map((f) => (
            <button
              key={f}
              class={f === filter ? "ws-activity-filter on" : "ws-activity-filter"}
              aria-pressed={f === filter}
              data-activity-filter={f}
              onClick={() => setPrefs({ filter: f, open: true })}
            >
              {LABEL[f]}
              <span class="ws-activity-count">{kept.filter((it) => inFilter(f, it.kind)).length}</span>
            </button>
          ))}
        </div>
        <span class="ws-activity-fill" />
        {p.open && (
          <>
            {waiting > 0 && (
              <button class="ws-activity-waiting" data-activity-waiting={waiting} onClick={refresh} title="Show them (Live is off)">
                {waiting} new
              </button>
            )}
            <button
              class={p.live ? "ws-activity-live on" : "ws-activity-live"}
              aria-pressed={p.live}
              data-activity-live
              title={p.live ? "Following new activity: click to hold the view still (it keeps collecting)" : "Follow new activity as it comes"}
              onClick={live}
            >
              <span class="ws-dot" aria-hidden="true" />
              Live
            </button>
            <button class="ws-activity-refresh" data-activity-refresh title="Read the session's history again" onClick={refresh}>
              Refresh
            </button>
          </>
        )}
        <button class="ws-activity-more" data-activity-more aria-label="Activity menu" title="Activity: clear this view, collapse" onClick={more}>
          <MoreIcon size={15} />
        </button>
      </header>
      {p.open && (
        <div
          class="ws-activity-list"
          ref={list}
          onScroll={(e) => {
            const el = e.currentTarget;
            atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
          }}
        >
          {feed.status === "loading" && shown.length === 0 ? (
            <div class="ws-activity-empty">Reading the session's history…</div>
          ) : feed.status === "denied" ? (
            <div class="ws-activity-empty" data-activity-denied>
              This session's activity isn't shared with you.
            </div>
          ) : feed.status === "error" && shown.length === 0 ? (
            <div class="ws-activity-empty error" data-activity-error>
              {feed.error}
              <button onClick={refresh}>Try again</button>
            </div>
          ) : shown.length === 0 ? (
            <div class="ws-activity-empty" data-activity-empty>
              {all.length > 0 && kept.length === all.length && cleared > 0
                ? "Cleared. New activity shows here."
                : filter === "system"
                  ? "Nothing yet: panes opening, closing and exiting show here."
                  : filter === "agents"
                    ? "No agent turns or answers in this session yet."
                    : filter === "commands"
                      ? "No commands in this session yet."
                      : "Nothing in this session yet."}
            </div>
          ) : (
            shown.map((it) => <Row key={it.id} client={client} it={it} onClick={() => reveal(it)} />)
          )}
          {feed.status === "error" && shown.length > 0 && (
            <div class="ws-activity-note error" data-activity-error>
              {feed.error}
            </div>
          )}
        </div>
      )}
    </section>
  );
}

function Row({ client, it, onClick }: { client: Client; it: Item; onClick: () => void }) {
  const here = it.pane !== null && !!client.info(it.pane);
  const status =
    it.kind === "system" || it.kind === "agent" || it.kind === "answer"
      ? null
      : it.ended === null
        ? { cls: "running", text: "running" }
        : it.exit === null || it.exit === 0
          ? { cls: "ok", text: "ok" }
          : { cls: "bad", text: `exit ${it.exit}` };
  return (
    <button
      class="ws-activity-row"
      data-activity-id={it.id}
      data-activity-kind={it.kind}
      data-activity-pane={it.pane ?? undefined}
      disabled={!here}
      title={here ? "Show its pane" : it.pane === null ? undefined : "Its pane is closed"}
      onClick={onClick}
    >
      <time class="ws-activity-time">{clock(it.at)}</time>
      <span class={`ws-activity-kind ${it.kind}`}>{KIND[it.kind]}</span>
      <span class="ws-activity-text">{it.text || "(no text)"}</span>
      <span class={`ws-activity-status ${status?.cls ?? ""}`}>{status?.text}</span>
      {it.by && (
        <span class="ws-activity-by" title={it.by}>
          {byName(it.by)}
        </span>
      )}
      {it.pane !== null && (
        <span class={here ? "ws-activity-pane" : "ws-activity-pane closed"} title={`Pane %${it.pane}`}>
          {here ? paneName(client, it.pane) : "closed pane"}
        </span>
      )}
    </button>
  );
}

const Chevron = ({ open }: { open: boolean }) => (
  <svg class="icon" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true">
    <path d={open ? "M6 9l6 6 6-6" : "M6 15l6-6 6 6"} stroke-linecap="round" stroke-linejoin="round" />
  </svg>
);

/** The drawer's top edge: drag it taller or shorter. The panes above keep
 * at least a few rows; the height is kept when you let go. */
function Resize() {
  const down = (e: PointerEvent) => {
    e.preventDefault();
    const el = e.currentTarget as HTMLElement;
    const drawer = el.parentElement!;
    const work = drawer.parentElement!;
    const y0 = e.clientY;
    const h0 = drawer.getBoundingClientRect().height;
    const max = Math.max(MIN_H, work.getBoundingClientRect().height - 160);
    el.setPointerCapture(e.pointerId);
    const move = (m: PointerEvent) =>
      setPrefs(
        {
          height: Math.round(Math.min(max, Math.max(MIN_H, h0 + y0 - m.clientY))),
        },
        false,
      );
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("pointercancel", up);
      setPrefs({});
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("pointercancel", up);
  };
  const key = (e: KeyboardEvent) => {
    const d = e.key === "ArrowUp" ? 24 : e.key === "ArrowDown" ? -24 : 0;
    if (!d) return;
    e.preventDefault();
    setPrefs({ height: Math.max(MIN_H, prefs.height + d) });
  };
  return (
    <div
      class="ws-activity-resize"
      role="separator"
      aria-orientation="horizontal"
      aria-label="Activity drawer height"
      tabIndex={0}
      onPointerDown={down}
      onKeyDown={key}
    />
  );
}

/** The footer's switch for the drawer: a small thing of its own, so the
 * pane toolbar (M110) and the footer can each place it. */
export function ActivityToggle({ client }: { client: Client }) {
  const p = usePrefs();
  if (unavailable.has(client)) return null;
  return (
    <button
      class={p.open ? "ws-activity-toggle on" : "ws-activity-toggle"}
      data-activity-toggle
      aria-pressed={p.open}
      title={p.open ? "Collapse the activity drawer" : "Open the activity drawer"}
      onClick={() => setActivityOpen(!p.open)}
    >
      <svg class="icon" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" aria-hidden="true">
        <path d="M3 12h4l3-8 4 16 3-8h4" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
      Activity
    </button>
  );
}
