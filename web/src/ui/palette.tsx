// The command palette (#139): Ctrl+Shift+P (Cmd+Shift+P on a Mac) from
// anywhere, a terminal pane included, lists every action the menus offer
// for the active pane, its tab and session, plus jumps to tabs, panes and
// workspaces and the swarm. Typing filters with the picker's fuzzy match;
// recent picks come first. On a phone it is a full-height sheet, opened
// from the sheet's Commands button.
//
// The menus' items come from ./commands, so anything added to a menu shows
// up here too. Blocks drawn in an iframe on another origin (web pages,
// editors, studio apps) keep their keys: the chord works once focus is
// back on the page.

import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import { paneIds, tabLabel, type Client } from "../client";
import type { PaneId } from "../proto";
import { fuzzy } from "../fs";
import { countView } from "../telemetry";
import { useWorkspaceDir } from "../blocks";
import { closeSwarm, openSwarm, swarmRoute } from "../swarm/route";
import { closeChat, isChatOpen, openChat, openChatView } from "./places";
import { agentsRoute, closeAgentsPage, openAgentsPage } from "./agents-route";
import { askText, closeMenu, type MenuItem } from "./menu";
import { ariaKeys, keyHint } from "./keys";
import { newTabItems, paneItems, sessionItems, settingsItems, tabItems } from "./commands";

export interface Command {
  /** Stable enough to remember as a recent pick. */
  key: string;
  group: string;
  label: string;
  run: () => unknown;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  checked?: boolean;
  /** Where it is, for jumps: the session, the tab. */
  detail?: string;
}

let open: { client: Client; phone: boolean } | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function openPalette(client: Client, phone = false) {
  countView("palette");
  closeMenu();
  open = { client, phone };
  changed();
}

function closePalette() {
  open = null;
  changed();
}

/** The chord (ui/keys.ts): opens it, and again closes it. */
export function togglePalette(client: Client, phone: boolean) {
  if (open) closePalette();
  else openPalette(client, phone);
}

export function PaletteLayer() {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => {
      listeners.delete(fn);
    };
  }, []);
  if (!open) return null;
  return <Palette client={open.client} phone={open.phone} close={closePalette} />;
}

// ---------------------------------------------------------------- recents

const RECENT_KEY = "arugula.palette.recent";
const RECENT_MAX = 8;

function recents(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((k): k is string => typeof k === "string") : [];
  } catch {
    return [];
  }
}

function remember(key: string) {
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify([key, ...recents().filter((k) => k !== key)].slice(0, RECENT_MAX)));
  } catch {
    // No storage (a private window): no recents.
  }
}

// ---------------------------------------------------------------- commands

/** A menu's items as commands: headers name the items after them, up to
 * the next separator; a header that opens the menu says what it's for. */
function fromMenu(group: string, items: MenuItem[], about?: string): Command[] {
  const out: Command[] = [];
  let under: string | null = null;
  let detail: string | undefined = about;
  items.forEach((item, i) => {
    if (item === "separator") under = null;
    else if ("header" in item) {
      if (i === 0) detail = item.header;
      else under = item.header;
    } else {
      const label = under ? `${under}: ${item.label}` : item.label;
      out.push({ key: `${group}/${label}`, group, detail, ...item, label });
    }
  });
  return out;
}

/** Every command for what's shown now. */
export function commands(client: Client, phone: boolean, workspace: string | null): Command[] {
  const state = client.state;
  if (!state) return [];
  const out: Command[] = [];
  const pane = client.active();
  const tab = client.tabView();
  const session = client.session;

  // Which pane each acts on, beside it: "Pane" alone says nothing.
  if (pane !== undefined) out.push(...fromMenu("Pane", paneItems(client, pane, phone, workspace), client.title(pane) || client.info(pane)?.command || `pane %${pane}`));
  if (tab) {
    const rename = async () => {
      const name = await askText("Rename tab", tab.name ?? tabLabel(client, tab));
      if (name !== null) client.intent({ op: "rename_tab", tab: tab.id, name: name.trim() || null });
    };
    out.push(...fromMenu("Tab", tabItems(client, tab, () => void rename())));
  }
  if (session !== null) {
    out.push(...fromMenu("New", newTabItems(client, session)));
    const s = state.sessions.find((x) => x.id === session);
    const rename = async () => {
      const name = await askText("Rename session", s?.name ?? "");
      if (name?.trim() && name.trim() !== s?.name) client.intent({ op: "rename_session", session, name: name.trim() });
    };
    out.push(...fromMenu("Session", sessionItems(client, session, () => void rename())));
  }

  // The account menu's settings: this machine's, this device's, help.
  // Without the menu's headers: each would name every item after it.
  out.push(...fromMenu("Settings", settingsItems(client).filter((i) => typeof i === "object" && "label" in i && i.label !== "Command palette…")));

  // Jumps: sessions, tabs, panes and workspaces by name.
  for (const s of state.sessions) {
    if (s.id !== session) out.push({ key: `Go/session ${s.id}`, group: "Go to", label: `Session ${s.name}`, run: () => client.selectSession(s.id) });
    for (const tid of s.tabs) {
      const t = client.tabView(tid);
      if (!t) continue;
      const label = tabLabel(client, t);
      const panes = paneIds(t);
      out.push({ key: `Go/tab ${tid}`, group: "Go to", label: `Tab ${label}`, detail: s.name, run: () => client.selectTab(tid) });
      for (const p of panes) {
        const info = client.info(p);
        if (info?.type === "workspace") {
          const name = client.title(p).replace(/ \(chant\)$/, "");
          out.push({ key: `Go/pane ${p}`, group: "Go to", label: `Workspace ${name}`, detail: `${s.name} › ${label}`, run: () => client.setActive(p) });
        } else if (panes.length > 1) {
          const name = client.title(p) || client.cwd(p) || `pane %${p}`;
          out.push({ key: `Go/pane ${p}`, group: "Go to", label: `Pane ${name}`, detail: `${s.name} › ${label}`, run: () => client.setActive(p) });
        }
      }
    }
  }

  // The swarm, and the next pane that wants you after the active one.
  const wanting = state.panes.filter((p) => p.attention === "needs_input").map((p) => p.id);
  const next: PaneId | undefined = wanting.find((p) => p > (pane ?? -1)) ?? wanting[0];
  out.push({ key: "Swarm/open", group: "Swarm", label: "Open the swarm", run: openSwarm });
  if (client.hasThreads()) out.push({ key: "Chat/open", group: "Chat", label: "Open chat: every thread", run: () => openChat() });
  out.push({
    key: "Swarm/next",
    group: "Swarm",
    label: "Next pane that needs you",
    disabled: next === undefined,
    detail: next === undefined ? "nothing needs you" : client.title(next) || `pane %${next}`,
    run: () => next !== undefined && client.setActive(next),
  });
  out.push(...places(client));
  out.push({
    key: "Swarm/next-in-swarm",
    group: "Swarm",
    label: "Show the next pane that needs you in the swarm",
    disabled: next === undefined,
    run: () => next !== undefined && (location.hash = `swarm=${next}`),
  });
  return out;
}

/** Where the palette was opened: the panes, or a place over them. */
function here(): "panes" | "swarm" | "chat" | "agents" {
  return swarmRoute() ? "swarm" : isChatOpen() ? "chat" : agentsRoute() ? "agents" : "panes";
}

/** The places, and what the one you're in offers: first when you're not
 * on the panes, so the palette starts with the view it's opened over. */
function places(client: Client): Command[] {
  const at = here();
  const toPanes = () => {
    if (swarmRoute()) closeSwarm();
    if (isChatOpen()) closeChat();
    if (agentsRoute()) closeAgentsPage();
  };
  const out: Command[] = [];
  const go = (key: string, label: string, run: () => void) => out.push({ key: `Places/${key}`, group: "Places", label, run });
  if (at !== "panes") go("panes", "Go to Panes", toPanes);
  if (at !== "swarm") go("swarm", "Go to Swarm", openSwarm);
  if (client.hasThreads()) {
    if (at !== "chat") go("chat", "Go to Chat", () => openChat());
    go("activity", "Chat: Activity, what needs you", () => openChatView("activity"));
  }
  if (client.flag("agents") && !client.state?.roles) {
    go("recipes", "Agents: Recipes", () => openAgentsPage("recipes"));
    go("team", "Agents: Team", () => openAgentsPage("team"));
  }
  return out;
}

function rank(list: Command[], query: string, recent: string[]): Command[] {
  // Over a place, its commands come first, before the pane's.
  if (here() !== "panes") list = [...list.filter((c) => c.group === "Places"), ...list.filter((c) => c.group !== "Places")];
  const q = query.trim();
  const at = (c: Command) => {
    const i = recent.indexOf(c.key);
    return i < 0 ? Infinity : i;
  };
  if (!q) {
    // Recent picks first, newest first; the rest as the menus have them.
    const first = list.filter((c) => at(c) < Infinity).sort((a, b) => at(a) - at(b));
    return [...first, ...list.filter((c) => at(c) === Infinity)];
  }
  const scored = list
    .map((c) => {
      const a = fuzzy(q, c.label);
      const b = fuzzy(q, `${c.group} ${c.label}`);
      const s = a === null ? b : b === null ? a : Math.max(a, b);
      return { c, s: s === null ? null : s + (at(c) < Infinity ? 4 : 0) - (c.disabled ? 20 : 0) };
    })
    .filter((x): x is { c: Command; s: number } => x.s !== null);
  scored.sort((a, b) => b.s - a.s);
  return scored.map((x) => x.c);
}

// ---------------------------------------------------------------- view

function Palette({ client, phone, close }: { client: Client; phone: boolean; close: () => void }) {
  const [query, setQuery] = useState("");
  const [sel, setSel] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const [recent] = useState(recents);
  const pane = client.active();
  const info = pane === undefined ? undefined : client.info(pane);
  const cwd = pane === undefined ? null : client.cwd(pane);
  const isWorkspace = useWorkspaceDir(client, pane ?? -1, info?.type === "terminal" ? cwd : null);
  // Re-read when the daemon's state changes (a pane closing, say).
  const [, setTick] = useState(0);
  useEffect(() => client.subscribe(() => setTick((t) => t + 1)), [client]);

  const rows = useMemo(
    () => rank(commands(client, phone, isWorkspace ? cwd : null), query, recent),
    [client, phone, isWorkspace, cwd, query, recent, client.state?.rev, pane],
  );

  // A phone's keyboard would cover half the list: only on request there.
  useLayoutEffect(() => {
    if (!phone) input.current?.focus();
  }, []);
  useEffect(() => {
    listRef.current?.querySelector(".palette-row.selected")?.scrollIntoView({ block: "nearest" });
  }, [sel, rows]);
  useEffect(() => setSel((s) => Math.min(s, Math.max(0, rows.length - 1))), [rows.length]);

  const run = (c: Command | undefined) => {
    if (!c || c.disabled) return;
    remember(c.key);
    close();
    // The panes are under the swarm, chat and agents: show them, unless
    // it's the swarm's or chat's, or a place's (it goes there itself).
    if (c.group !== "Swarm" && c.group !== "Places" && swarmRoute()) closeSwarm();
    if (c.group !== "Swarm" && c.group !== "Chat" && c.group !== "Places") {
      if (isChatOpen()) closeChat();
      if (agentsRoute()) closeAgentsPage();
    }
    // Keys back to the pane first; a prompt the command opens takes them.
    const active = client.active();
    if (!phone && active !== undefined) client.viewOf(active)?.focus();
    void c.run();
  };

  const onKey = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      close();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setSel((s) => Math.min(rows.length - 1, s + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSel((s) => Math.max(0, s - 1));
    } else if (e.key === "Enter") {
      e.preventDefault();
      run(rows[sel]);
    }
  };

  return (
    <div
      class="prompt-backdrop picker-backdrop"
      role="dialog"
      aria-label="Command palette"
      onPointerDown={(e) => e.target === e.currentTarget && close()}
    >
      <div class={phone ? "picker palette phone" : "picker palette"} onKeyDown={onKey}>
        <input
          ref={input}
          class="picker-filter"
          placeholder="Type a command, a tab or a pane"
          aria-label="Command"
          aria-keyshortcuts={ariaKeys(keyHint("palette"))}
          value={query}
          autocomplete="off"
          autocapitalize="off"
          spellcheck={false}
          onInput={(e) => {
            setQuery(e.currentTarget.value);
            setSel(0);
          }}
        />
        <ul class="picker-list" role="listbox" aria-label="Commands" ref={listRef}>
          {rows.length === 0 && <li class="picker-empty">Nothing matches</li>}
          {rows.map((c, i) => (
            <li
              key={c.key}
              role="option"
              aria-selected={i === sel}
              aria-disabled={c.disabled}
              class={`picker-row palette-row${i === sel ? " selected" : ""}${c.disabled ? " disabled" : ""}${c.danger ? " danger" : ""}`}
              data-command={c.key}
              title={c.detail}
              onPointerMove={() => i !== sel && setSel(i)}
              onClick={() => run(c)}
            >
              <span class="palette-group">{c.group}</span>
              <span class="picker-label">
                {c.checked !== undefined && <span class="menu-check">{c.checked ? "●" : ""}</span>}
                {c.label}
              </span>
              {c.detail && <span class="palette-detail">{c.detail}</span>}
              {recent.includes(c.key) && !query && <span class="palette-recent">recent</span>}
              {c.shortcut && <kbd class="menu-key">{c.shortcut}</kbd>}
            </li>
          ))}
        </ul>
        {phone && (
          <div class="picker-actions">
            <button onClick={close}>Cancel</button>
          </div>
        )}
      </div>
    </div>
  );
}
