// The sessions on the shown machine. With the new workspace (M108, #1023)
// they're a sidebar beside the panes, grouped by what they need from you;
// in the classic bar they're the session button, whose menu switches
// sessions and holds the session's commands.

import { Fragment } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { cloudPanes, cloudRoute } from "../../cloud";
import { paneIds, type Client } from "../../client";
import { directory } from "../../hosts";
import type { Attention, PaneInfo, Session, SessionId, TabId } from "../../proto";
import { AttentionBadge } from "../attention";
import { sessionItems } from "../commands";
import { openFresh } from "../open-fresh";
import { closePlace, currentPlace } from "../places";
import { BranchIcon, MoreIcon, PanesIcon, PlusIcon, SearchIcon, TabsIcon } from "./icons";
import { setSidebarOpen, setSidebarWidth, takeSearchFocus, useSidebar } from "./prefs";
import { RenameInput, type Renaming } from "./rename-input";

// ---- what a session row says, from real state only

type Group = "needs" | "working" | "idle";
const GROUPS: { id: Group; label: string }[] = [
  { id: "needs", label: "Needs you" },
  { id: "working", label: "Working" },
  { id: "idle", label: "Idle" },
];

/** M109 adds `PaneInfo.branch`; until it lands (and from an older daemon)
 * there's none, and nothing is shown for it. */
const branchOf = (p: PaneInfo): string | null => (p as PaneInfo & { branch?: string | null }).branch ?? null;

export interface SessionSummary {
  session: Session;
  tabs: number;
  panes: number;
  attention: Attention;
  /** The pane that wants you most, for the badge's menu. */
  pane?: number;
  reason?: PaneInfo["reason"];
  group: Group;
  /** The repo and branch when every pane that has one agrees; "Multiple
   * branches" (or repos) when they don't; null when none is known. */
  context: string | null;
  /** When any of its panes last printed (ms), 0 if unknown. */
  last: number;
}

export function summarize(client: Client, session: Session): SessionSummary {
  const tabs = session.tabs.flatMap((id) => {
    const t = client.tabView(id);
    return t ? [t] : [];
  });
  const infos = tabs.flatMap((t) => paneIds(t)).flatMap((p) => {
    const i = client.info(p);
    return i ? [i] : [];
  });
  const pick = (s: Attention) => infos.find((i) => i.attention === s);
  const top = pick("needs_input") ?? pick("working") ?? pick("done");
  const attention = top?.attention ?? "idle";
  const repos = [...new Set(infos.flatMap((i) => (i.project ? [i.project.name] : [])))];
  const branches = [...new Set(infos.flatMap((i) => (branchOf(i) ? [branchOf(i)!] : [])))];
  let context: string | null = null;
  if (branches.length > 1) context = "Multiple branches";
  else if (repos.length > 1) context = "Multiple repos";
  else if (repos.length === 1 || branches.length === 1) context = [repos[0], branches[0]].filter(Boolean).join(" · ");
  return {
    session,
    tabs: tabs.length,
    panes: infos.length,
    attention,
    pane: top?.id,
    reason: top?.reason,
    group: attention === "needs_input" ? "needs" : attention === "working" ? "working" : "idle",
    context,
    last: Math.max(0, ...infos.map((i) => i.activity?.last_ms ?? 0)),
  };
}

function ago(ms: number): string | null {
  if (!ms) return null;
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 60) return "now";
  if (s < 3600) return `${Math.round(s / 60)}m ago`;
  if (s < 86400) return `${Math.round(s / 3600)}h ago`;
  return `${Math.round(s / 86400)}d ago`;
}

// ---- the session's last tab, so going back to it lands where you were

const lastTabs = new Map<string, TabId>();
const lastKey = (client: Client, s: SessionId) => `${client.base}\u0000${s}`;

/** Note the shown session's tab (the workspace calls this as it draws). */
export function noteLastTab(client: Client) {
  if (client.session !== null && client.tab !== null) lastTabs.set(lastKey(client, client.session), client.tab);
}

/** Show a session at its last tab (and that tab's last pane), or its first. */
export function showSession(client: Client, id: SessionId) {
  cloudPanes.local();
  const at = currentPlace();
  if (at !== "panes") closePlace(at);
  const last = lastTabs.get(lastKey(client, id));
  const session = client.state?.sessions.find((s) => s.id === id);
  if (last !== undefined && session?.tabs.includes(last)) client.selectTab(last);
  else client.selectSession(id);
}

// ---- the sidebar

export function SessionSidebar({
  client,
  renaming,
  setRenaming,
}: {
  client: Client;
  renaming: Renaming;
  setRenaming: (r: Renaming) => void;
}) {
  const { open, width, narrow } = useSidebar();
  const [query, setQuery] = useState("");
  // The highlighted row by id, not position: rows move between groups as
  // their panes start and stop working.
  const [hiPick, setHi] = useState<SessionId | null>(null);
  const search = useRef<HTMLInputElement>(null);
  const state = client.state!;
  // Re-render each minute too, for "2m ago".
  const [, tick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => tick((n) => n + 1), 30_000);
    return () => clearInterval(t);
  }, []);
  useLayoutEffect(() => {
    if (open && takeSearchFocus()) search.current?.focus();
  });
  const all = state.sessions.map((s) => summarize(client, s));
  const q = query.trim().toLowerCase();
  const shown = q ? all.filter((r) => r.session.name.toLowerCase().includes(q) || (r.context ?? "").toLowerCase().includes(q)) : all;
  // In the order they're drawn, for the arrow keys.
  const ordered = GROUPS.flatMap((g) => shown.filter((r) => r.group === g.id));
  const hiId = ordered.some((r) => r.session.id === hiPick) ? hiPick! : ordered[0]?.session.id;
  if (!open) return null;
  const choose = (id: SessionId) => {
    showSession(client, id);
    if (narrow) setSidebarOpen(false);
  };
  const create = () => {
    cloudPanes.local();
    client.intent({ op: "new_session", name: null, from_pane: client.active() ?? null });
  };
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!ordered.length) return;
      const i = Math.max(0, ordered.findIndex((r) => r.session.id === hiId));
      setHi(ordered[(i + (e.key === "ArrowDown" ? 1 : ordered.length - 1)) % ordered.length].session.id);
    } else if (e.key === "Enter") {
      if (hiId !== undefined) {
        e.preventDefault();
        choose(hiId);
        setQuery("");
        client.viewOf(client.active() ?? -1)?.focus();
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      if (query) {
        setHi(null);
        setQuery("");
      }
      else {
        if (narrow) setSidebarOpen(false);
        client.viewOf(client.active() ?? -1)?.focus();
      }
    }
  };
  const host = directory.current || "this machine";
  return (
    <aside class={narrow ? "ws-sidebar overlay" : "ws-sidebar"} style={{ width: `${width}px` }} aria-label={`Sessions on ${host}`} data-session-sidebar>
      <div class="ws-sidebar-head">
        <h2>Sessions</h2>
        <span class="ws-sidebar-count">{all.length}</span>
        <button class="ws-icon-button" aria-label="New session" title={`New session on ${host}`} data-new-session onClick={create}>
          <PlusIcon />
        </button>
      </div>
      <label class="ws-search">
        <SearchIcon size={14} />
        <input
          ref={search}
          type="search"
          placeholder="Search sessions"
          aria-label="Search sessions"
          aria-controls="ws-session-list"
          aria-activedescendant={q && hiId !== undefined ? `ws-session-${hiId}` : undefined}
          value={query}
          data-session-search
          onInput={(e) => {
            // A new search starts at its first match (here, not in an
            // effect, which can run after the next key).
            setHi(null);
            setQuery((e.currentTarget as HTMLInputElement).value);
          }}
          onKeyDown={onKey}
        />
      </label>
      <div class="ws-session-list" id="ws-session-list" role="listbox" aria-label="Sessions">
        {GROUPS.map((g) => {
          const rows = shown.filter((r) => r.group === g.id);
          if (!rows.length) return null;
          return (
            <Fragment key={g.id}>
              <h3 class={`ws-group ${g.id}`} data-session-group={g.id}>
                {g.label} <span class="ws-group-count">{rows.length}</span>
              </h3>
              {rows.map((r) => (
                <SessionRow
                  key={r.session.id}
                  client={client}
                  row={r}
                  current={r.session.id === client.session && !cloudRoute()}
                  hi={!!q && r.session.id === hiId}
                  renaming={renaming?.kind === "session" && renaming.id === r.session.id}
                  setRenaming={setRenaming}
                  choose={choose}
                />
              ))}
            </Fragment>
          );
        })}
        {shown.length === 0 && (
          <div class="ws-empty" data-session-empty role="status">
            {q ? (
              <>
                <p>No session on {host} matches “{query.trim()}”.</p>
                <button onClick={() => setQuery("")}>Clear the search</button>
              </>
            ) : (
              <p>No sessions on {host}.</p>
            )}
          </div>
        )}
      </div>
      {!narrow && <Resizer width={width} />}
    </aside>
  );
}

function SessionRow({
  client,
  row,
  current,
  hi,
  renaming,
  setRenaming,
  choose,
}: {
  client: Client;
  row: SessionSummary;
  current: boolean;
  hi: boolean;
  renaming: boolean;
  setRenaming: (r: Renaming) => void;
  choose: (id: SessionId) => void;
}) {
  const s = row.session;
  const menu = (e: MouseEvent) => {
    e.stopPropagation();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    openFresh(client, { clientX: r.right, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, () =>
      sessionItems(client, s.id, () => setRenaming({ kind: "session", id: s.id })),
    );
  };
  const when = ago(row.last);
  const unread = client.thread({ session: s.id });
  return (
    <div
      id={`ws-session-${s.id}`}
      class={`ws-session${current ? " current" : ""}${hi ? " hi" : ""}`}
      role="option"
      aria-selected={current}
      data-session-id={s.id}
      data-attention={row.attention}
      onContextMenu={menu}
    >
      {renaming ? (
        <RenameInput
          value={s.name}
          onDone={(name) => {
            setRenaming(null);
            if (name && name !== s.name) client.intent({ op: "rename_session", session: s.id, name });
          }}
        />
      ) : (
        <button class="ws-session-main" title={`${s.name}: ${row.tabs} tab${row.tabs === 1 ? "" : "s"}, ${row.panes} pane${row.panes === 1 ? "" : "s"}`} onClick={() => choose(s.id)}>
          <span class="ws-session-line">
            <span class={`ws-dot ${row.attention}`} aria-hidden="true" />
            <span class="ws-session-name">{s.name}</span>
            {unread?.unread ? <span class={unread.mention ? "session-unread mention" : "session-unread"} title="New in the session thread" /> : null}
            <span class="ws-session-counts" aria-label={`${row.tabs} tabs, ${row.panes} panes`}>
              <span title={`${row.tabs} tab${row.tabs === 1 ? "" : "s"}`}>
                <TabsIcon /> {row.tabs}
              </span>
              <span title={`${row.panes} pane${row.panes === 1 ? "" : "s"}`}>
                <PanesIcon size={12} /> {row.panes}
              </span>
            </span>
          </span>
          {(row.context || when) && (
            <span class="ws-session-sub">
              {row.context ? (
                <span class="ws-session-context">
                  {row.context.startsWith("Multiple") ? null : <BranchIcon size={11} />}
                  {row.context}
                </span>
              ) : (
                <span />
              )}
              {when && <span class="ws-session-ago">{when}</span>}
            </span>
          )}
        </button>
      )}
      <AttentionBadge state={row.attention} reason={row.reason} pane={row.pane} client={client} />
      <button class="ws-icon-button ws-session-more" aria-label={`Actions for ${s.name}`} title="Rename, share, notifications, close" onClick={menu}>
        <MoreIcon />
      </button>
    </div>
  );
}

/** The sidebar's right edge: drag to resize, kept on this device. */
function Resizer({ width }: { width: number }) {
  return (
    <div
      class="ws-sidebar-resize"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the sessions"
      aria-valuenow={width}
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft" || e.key === "ArrowRight") {
          e.preventDefault();
          setSidebarWidth(width + (e.key === "ArrowRight" ? 16 : -16), true);
        }
      }}
      onPointerDown={(e) => {
        const el = e.currentTarget as HTMLElement;
        el.setPointerCapture(e.pointerId);
        const x0 = e.clientX;
        const w0 = width;
        const move = (m: PointerEvent) => setSidebarWidth(w0 + m.clientX - x0, false);
        const up = (u: PointerEvent) => {
          setSidebarWidth(w0 + u.clientX - x0, true);
          el.removeEventListener("pointermove", move);
          el.removeEventListener("pointerup", up);
        };
        el.addEventListener("pointermove", move);
        el.addEventListener("pointerup", up);
      }}
    />
  );
}

/** The session breadcrumb in the new header, or the classic bar's session
 * button: its menu switches sessions and holds the session's commands.
 * `crumb` names it without the "Session" label, and never renames in
 * place (the sidebar's row does). */
export function SessionButton({
  client,
  session,
  renaming,
  setRenaming,
  crumb,
}: {
  client: Client;
  session: Session;
  renaming: Renaming;
  setRenaming: (r: Renaming) => void;
  crumb?: boolean;
}) {
  const state = client.state!;
  const sessionMenu = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const rename = () => {
      if (crumb) setSidebarOpen(true);
      setRenaming({ kind: "session", id: session.id });
    };
    openFresh(client, { clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, () => [
      ...state.sessions.map((s) => ({
        label: `${s.id === session.id ? "✓ " : "    "}${s.name}`,
        run: () => (crumb ? showSession(client, s.id) : client.selectSession(s.id)),
      })),
      "separator",
      ...sessionItems(client, session.id, rename, { bar: true }),
    ]);
  };
  if (!crumb && renaming?.kind === "session" && renaming.id === session.id) {
    return (
      <RenameInput
        value={session.name}
        onDone={(name) => {
          setRenaming(null);
          if (name && name !== session.name) client.intent({ op: "rename_session", session: session.id, name });
        }}
      />
    );
  }
  return (
    <button
      class={crumb ? "session-button ws-crumb" : "session-button"}
      title={crumb ? "This session: switch sessions, rename, close" : "Sessions"}
      aria-label={`Session: ${session.name}`}
      onClick={sessionMenu}
      onContextMenu={sessionMenu}
    >
      {!crumb && <span class="session-label">Session</span>}
      {crumb ? <span class="ws-crumb-name">{session.name}</span> : session.name}
      {/* M61: the session's thread has messages you haven't read. */}
      {client.thread({ session: session.id })?.unread ? (
        <span class={client.thread({ session: session.id })?.mention ? "session-unread mention" : "session-unread"} title="New in the session thread" />
      ) : null}{" "}
      <span class="caret">▾</span>
    </button>
  );
}
