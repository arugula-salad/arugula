// Getting around the chat page (M75): Ctrl/Cmd+K to jump to a channel,
// search over every machine's threads, Activity (your mentions and the
// agents' answers to you), and keys to move through the channels.
//
// Search and Activity read the threads through each machine's own
// connection, as the person reading may: the daemon's `/api/search` is the
// owner's alone, and covers output too.

import { useEffect, useRef, useState } from "preact/hooks";
import { fuzzy } from "../fs";
import { threadKey, type ThreadMsg, type ThreadSummary, type ThreadTarget } from "../proto";
import { closeChat, openChat, openChatView, type ChatRoute, type Source } from "./places";
import { getFleet } from "./hosts";
import { useSubscribe } from "./hooks";
import type { Fleet, FleetPane } from "../fleet";
import { cardTitle, holdStill, reasonOf, shortHost, shortPaths, shownNow, wantsYou, whatItDoes } from "../swarm/model";
import { paneLabel, sessionName, type Row } from "./chat";
import { Markup } from "./markup";
import { MsgAvatar } from "./threads";

// ---- every thread's messages, loaded as they're needed

interface Indexed {
  s: Source;
  target: ThreadTarget;
  summary: ThreadSummary;
  msgs: ThreadMsg[];
}

const cache = new Map<string, { last: number; msgs: ThreadMsg[] }>();
const loading = new Set<string>();
const indexListeners = new Set<() => void>();

/** Every thread on every machine with what's loaded of it; a thread that
 * changed (its last message) loads again. */
function indexOf(all: Source[]): Indexed[] {
  const out: Indexed[] = [];
  for (const s of all) {
    for (const summary of s.client.state?.threads ?? []) {
      const key = `${s.host}/${threadKey(summary.target)}`;
      const c = cache.get(key);
      if ((!c || c.last !== summary.last) && !loading.has(key)) {
        loading.add(key);
        s.client
          .loadThread(summary.target)
          .then(
            (msgs) => cache.set(key, { last: summary.last, msgs }),
            // Gone, or not ours to read: nothing to show.
            () => cache.set(key, { last: summary.last, msgs: [] }),
          )
          .finally(() => {
            loading.delete(key);
            indexListeners.forEach((fn) => fn());
          });
      }
      out.push({ s, target: summary.target, summary, msgs: c?.msgs ?? [] });
    }
  }
  return out;
}

function useIndex(all: Source[]): { index: Indexed[]; busy: boolean } {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    indexListeners.add(fn);
    // Loads this render started may have finished before we listened.
    fn();
    return () => void indexListeners.delete(fn);
  }, []);
  const index = indexOf(all);
  return { index, busy: loading.size > 0 };
}

// ---- results: a message, where it is, and a way there

interface Hit {
  s: Source;
  target: ThreadTarget;
  m: ThreadMsg;
  unread: boolean;
}

function channelName(s: Source, target: ThreadTarget, multi: boolean): string {
  const c = s.client;
  const name = "pane" in target ? `↳ ${paneLabel(c, target.pane)}` : `# ${sessionName(c, target.session)}`;
  return multi ? `${name} · ${s.host || "this machine"}` : name;
}

function when(at: number): string {
  const d = new Date(at);
  return new Date().toDateString() === d.toDateString()
    ? d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })
    : d.toLocaleString([], { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

function HitList({ hits, multi }: { hits: Hit[]; multi: boolean }) {
  return (
    <ul class="chat-hits">
      {hits.map((h) => (
        <li key={`${h.s.host}/${threadKey(h.target)}/${h.m.id}`}>
          <button class={h.unread ? "chat-hit unread" : "chat-hit"} data-chat-hit={h.m.id} onClick={() => openChat(h.s.host, h.target, h.m.id)}>
            <span class="chat-hit-where">
              <span>{channelName(h.s, h.target, multi)}</span>
              <time>{when(h.m.at)}</time>
            </span>
            <span class="chat-hit-body">
              {h.m.agent ? <span class="msg-avatar agent">⚙</span> : <MsgAvatar who={h.m.who} name={h.m.name} pic={h.m.pic} />}
              <span class="chat-hit-text">
                <b>{h.m.agent ? h.m.name : h.m.name.split("@")[0]}</b>
                {h.m.text ? <Markup text={h.m.text} /> : <p class="thread-text">{h.m.quote?.text}</p>}
              </span>
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}

function ViewHead({ title, sub, phone }: { title: string; sub?: string; phone: boolean }) {
  return (
    <header class="chat-head">
      {phone && (
        <button class="chat-back" title="Every thread" onClick={() => openChat()}>
          ‹
        </button>
      )}
      <div class="chat-title">
        <strong>{title}</strong>
        {sub && <span>{sub}</span>}
      </div>
    </header>
  );
}

// ---- Activity

/** Threads with a mention of you that you haven't read: the sidebar's count. */
export function activityCount(all: Source[]): number {
  let n = needing(getFleet()).length;
  for (const s of all) for (const t of s.client.state?.threads ?? []) if (t.mention && t.unread) n++;
  return n;
}

/** Messages for you: the ones that @mention you, and an agent's answer
 * after you @mentioned it. */
function activity(index: Indexed[]): Hit[] {
  const out: Hit[] = [];
  for (const { s, target, summary, msgs } of index) {
    const me = s.client.me();
    const firstUnread = summary.unread ? summary.last - summary.unread + 1 : Infinity;
    let asked = false;
    for (const m of msgs) {
      if (m.who === me && m.to_agent) asked = true;
      else if (m.agent && asked) {
        out.push({ s, target, m, unread: m.id >= firstUnread });
        asked = false;
      } else if (m.mentions?.includes(me)) out.push({ s, target, m, unread: m.id >= firstUnread });
    }
  }
  return out.sort((a, b) => b.m.at - a.m.at).slice(0, 100);
}

export function ActivityView({ all, multi, phone }: { all: Source[]; multi: boolean; phone: boolean }) {
  const { index, busy } = useIndex(all);
  const hits = activity(index);
  const fleet = getFleet();
  useSubscribe((fn) => fleet?.subscribe(fn) ?? (() => {}));
  const needs = needing(fleet);
  // On show here: a finished one leaves after 15 s, as on the swarm's rail.
  shownNow(needs);
  const [, tick] = useState(0);
  const finished = needs.some((p) => reasonOf(p)?.kind === "done");
  useEffect(() => {
    if (!finished) return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [finished]);
  return (
    <section class="chat-thread chat-view" aria-label="Activity" data-chat-activity-view>
      <ViewHead title="Activity" sub="What needs you on every machine, mentions of you, and agents' answers to you" phone={phone} />
      <div class="thread-list">
        {fleet && needs.length > 0 && <NeedsYou fleet={fleet} panes={needs} />}
        {hits.length === 0 && needs.length === 0 && (
          <p class="thread-empty">
            {busy ? "Loading…" : "Nothing for you yet. When an agent asks you something or finishes, when a command fails, or when someone @mentions you, it's here."}
          </p>
        )}
        {hits.length > 0 && needs.length > 0 && <h3 class="activity-head">Messages</h3>}
        <HitList hits={hits} multi={multi} />
      </div>
    </section>
  );
}

/** Every pane on every machine that wants you: what the swarm's rail
 * shows, so Chat isn't quiet while agents ask and finish. */
function needing(fleet: Fleet | null): FleetPane[] {
  return (fleet?.panes ?? []).filter((p) => wantsYou(p)).sort((a, b) => (reasonOf(b)!.since_ms ?? 0) - (reasonOf(a)!.since_ms ?? 0));
}

function NeedsYou({ fleet, panes }: { fleet: Fleet; panes: FleetPane[] }) {
  const [err, setErr] = useState("");

  const dismiss = async (p: FleetPane) => {
    acted.current.add(p.key);
    setErr("");
    const res = await fleet.request(p.host, "POST", "/api/attention/act", { action: "dismiss", pane: p.id }).catch((e: Error) => e);
    if (res instanceof Error) setErr(res.message);
    else if (!res.ok) setErr(`couldn't dismiss (${res.status})`);
  };
  // Rows hold still under the pointer, as the swarm's cards do.
  const [held, setHeld] = useState(false);
  const kept = useRef<(FleetPane & { gone?: boolean })[]>([]);
  const acted = useRef(new Set<string>());
  const rows = holdStill<FleetPane & { gone?: boolean }>(held, kept.current, panes, (p) => ({ ...p, gone: true }), acted.current);
  kept.current = rows;
  return (
    <div class="activity-needs" data-activity-needs onPointerEnter={() => setHeld(true)} onPointerLeave={() => setHeld(false)}>
      <h3 class="activity-head">Needs you</h3>
      {rows.map((p) => {
        const r = p.gone ? p.info.reason : reasonOf(p);
        if (!r) return null;
        const owner = fleet.host(p.host)?.owner;
        return (
          <div key={p.key} class={p.gone ? "activity-need gone" : "activity-need"} inert={p.gone} data-need={p.key} data-kind={r.kind}>
            <div class="activity-need-text">
              <b>{cardTitle(r, 1, [p.host])}</b>
              <span class="dim" title={`%${p.id} on ${p.host}`}>
                {" "}
                · {owner ? `${owner}'s · ` : ""}
                {whatItDoes(p)} · {shortHost(p.host)}
                {p.info.project?.name ? ` · ${p.info.project.name}` : ""}
              </span>
              <p title={r.headline}>{shortPaths(r.headline)}</p>
            </div>
            <div class="activity-need-buttons">
              <button
                class="pri"
                onClick={() => {
                  closeChat();
                  fleet.open(p.host, p.id);
                }}
              >
                Open
              </button>
              <button onClick={() => void dismiss(p)}>Dismiss</button>
            </div>
          </div>
        );
      })}
      {err && <p class="control-error">{err}</p>}
    </div>
  );
}

// ---- search

function search(index: Indexed[], q: string): Hit[] {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  if (!words.length) return [];
  const out: Hit[] = [];
  for (const { s, target, summary, msgs } of index) {
    const firstUnread = summary.unread ? summary.last - summary.unread + 1 : Infinity;
    for (const m of msgs) {
      const text = `${m.name} ${m.text} ${m.quote?.text ?? ""}`.toLowerCase();
      if (words.every((w) => text.includes(w))) out.push({ s, target, m, unread: m.id >= firstUnread });
    }
  }
  return out.sort((a, b) => b.m.at - a.m.at).slice(0, 200);
}

export function SearchView({ all, q, multi, phone }: { all: Source[]; q: string; multi: boolean; phone: boolean }) {
  const { index, busy } = useIndex(all);
  const hits = search(index, q);
  return (
    <section class="chat-thread chat-view" aria-label="Search" data-chat-search-view>
      <ViewHead
        title={q ? `Results for “${q}”` : "Search"}
        sub={q ? `${hits.length === 200 ? "200+" : hits.length} ${hits.length === 1 ? "message" : "messages"}${busy ? ", still looking…" : ""}` : undefined}
        phone={phone}
      />
      {phone && <SearchField route={{ view: "search", q }} />}
      <div class="thread-list">
        {!q && <p class="thread-empty">Type to search every thread on every machine.</p>}
        {q && hits.length === 0 && !busy && <p class="thread-empty">No messages match.</p>}
        <HitList hits={hits} multi={multi} />
      </div>
    </section>
  );
}

/** The bar's search field: results show as you type. */
export function SearchField({ route }: { route: ChatRoute }) {
  const shown = route.view === "search" ? (route.q ?? "") : "";
  const [text, setText] = useState(shown);
  const timer = useRef<number | undefined>(undefined);
  const input = useRef<HTMLInputElement>(null);
  // Leaving the results (a channel picked) clears it.
  useEffect(() => {
    if (route.view !== "search") setText("");
  }, [route.view]);
  useEffect(() => () => clearTimeout(timer.current), []);
  const go = (v: string, now = false) => {
    clearTimeout(timer.current);
    const run = () => (v.trim() ? openChatView("search", v.trim()) : route.view === "search" && openChat());
    if (now) run();
    else timer.current = window.setTimeout(run, 200);
  };
  return (
    <input
      ref={input}
      class="chat-search"
      type="search"
      placeholder="Search chat"
      aria-label="Search chat"
      data-chat-search
      value={text}
      onInput={(e) => {
        const v = (e.target as HTMLInputElement).value;
        setText(v);
        go(v);
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") go(text, true);
        if (e.key === "Escape") {
          setText("");
          go("", true);
          input.current?.blur();
        }
      }}
    />
  );
}

// ---- the switcher (Ctrl/Cmd+K)

let switcherOpen = false;
const switcherListeners = new Set<() => void>();

export function openSwitcher() {
  switcherOpen = true;
  switcherListeners.forEach((fn) => fn());
}

function closeSwitcher() {
  switcherOpen = false;
  switcherListeners.forEach((fn) => fn());
}

interface Channel {
  s: Source;
  r: Row;
}

/** The channels for `q`: by match, then unread, then newest. */
function ranked(rows: Channel[], q: string, multi: boolean): Channel[] {
  const scored = rows
    .map((c) => ({ c, score: fuzzy(q, `${c.r.label}${multi ? ` ${c.s.host}` : ""}`) }))
    .filter((x): x is { c: Channel; score: number } => x.score !== null);
  return scored
    .sort(
      (a, b) =>
        b.score - a.score ||
        (b.c.r.summary?.unread ?? 0) - (a.c.r.summary?.unread ?? 0) ||
        (b.c.r.summary?.at ?? 0) - (a.c.r.summary?.at ?? 0),
    )
    .map((x) => x.c)
    .slice(0, 12);
}

export function Switcher({ rows, multi }: { rows: Channel[]; multi: boolean }) {
  const [, setTick] = useState(0);
  const [q, setQ] = useState("");
  const [pick, setPick] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    const fn = () => {
      setTick((t) => t + 1);
      setQ("");
      setPick(0);
    };
    switcherListeners.add(fn);
    return () => {
      switcherListeners.delete(fn);
      switcherOpen = false;
    };
  }, []);
  useEffect(() => {
    if (switcherOpen) input.current?.focus();
  });
  if (!switcherOpen) return null;
  const list = ranked(rows, q, multi);
  const go = (c: Channel | undefined) => {
    closeSwitcher();
    if (c) openChat(c.r.host, c.r.target);
  };
  return (
    <div class="chat-switcher-back" onMouseDown={(e) => e.target === e.currentTarget && closeSwitcher()}>
      <div class="chat-switcher" role="dialog" aria-label="Jump to a session" data-chat-switcher>
        <input
          ref={input}
          placeholder="Jump to…"
          value={q}
          onInput={(e) => {
            setQ((e.target as HTMLInputElement).value);
            setPick(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              closeSwitcher();
            } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              if (list.length) setPick((p) => (p + (e.key === "ArrowDown" ? 1 : list.length - 1)) % list.length);
            } else if (e.key === "Enter") {
              e.preventDefault();
              go(list[pick]);
            }
          }}
        />
        <ul role="listbox">
          {list.map((c, i) => (
            <li
              key={`${c.r.host}/${threadKey(c.r.target)}`}
              role="option"
              aria-selected={i === pick}
              class={`${i === pick ? "picked" : ""}${c.r.summary?.unread ? " unread" : ""}`}
              onMouseDown={(e) => {
                e.preventDefault();
                go(c);
              }}
            >
              <span class="chat-sigil">{"pane" in c.r.target ? "↳" : "#"}</span>
              <span class="chat-label">{c.r.label}</span>
              {multi && <span class="chat-switcher-host">{c.s.host || "this machine"}</span>}
              {c.r.summary?.unread ? <span class={c.r.summary.mention ? "chat-count mention" : "chat-count"}>{c.r.summary.unread}</span> : null}
            </li>
          ))}
          {list.length === 0 && <li class="chat-none">No session matches.</li>}
        </ul>
      </div>
    </div>
  );
}

// ---- keys

/** Ctrl/Cmd+K: the switcher. Alt+↑/↓: the channel above or below;
 * with Shift, the next unread one. Shift+Esc: everything read. */
export function useChatKeys(rows: Channel[], pickedKey: string | null, markAll: () => void) {
  const latest = useRef({ rows, pickedKey, markAll });
  latest.current = { rows, pickedKey, markAll };
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (document.querySelector(".menu, .prompt")) return;
      const { rows, pickedKey, markAll } = latest.current;
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        e.stopPropagation();
        openSwitcher();
        return;
      }
      if (e.key === "Escape" && e.shiftKey) {
        e.preventDefault();
        markAll();
        return;
      }
      if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown") || !rows.length) return;
      e.preventDefault();
      const n = rows.length;
      const at = rows.findIndex((c) => `${c.r.host}/${threadKey(c.r.target)}` === pickedKey);
      const down = e.key === "ArrowDown";
      for (let i = 1; i <= n; i++) {
        // From the channel shown; with none, from just outside the list.
        const idx = at < 0 ? (down ? i - 1 : n - i) : (((at + (down ? i : -i)) % n) + n) % n;
        const c = rows[idx];
        if (idx === at) return;
        if (!e.shiftKey || c.r.summary?.unread) return openChat(c.r.host, c.r.target);
      }
    };
    addEventListener("keydown", key, true);
    return () => removeEventListener("keydown", key, true);
  }, []);
}
