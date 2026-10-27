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
import type { PaneId } from "../../shared/proto";
import { fuzzy } from "../../shared/fuzzy";
import { countView } from "../../shared/telemetry";
import { useWorkspaceDir } from "../blocks";
import { SWARM_ABOUT, closeSwarm, openSwarm, swarmRoute } from "../swarm/route";
import { closeOverview, hasChat, openChat, openChatView, openOverview, overviewRoute } from "./places";
import { showSession } from "./shell/session-sidebar";
import { SearchIcon, PanesIcon, SettingsIcon, SessionsIcon, ChatIcon, SwarmIcon, DocsIcon, AgentsIcon } from "./shell/icons";
import { paneName } from "./shell/overview";
import { openAttention } from "./shell/attention-drawer";
import { focusSessionSearch } from "./shell/prefs";
import { agentsRoute, closeAgentsPage, openAgentsPage } from "./agents-route";
import { closeDocsPage, docsRoute, openDoc, openDocsPage } from "./docs-route";
import { opPath, OPS } from "../ops";
import type { DocRow } from "../../shared/proto";
import { askText, closeMenu, type MenuItem } from "../../shared/ui/menu";
import { ariaKeys, keyHint } from "./keys";
import { newTabItems, paneItems, sessionItems, settingsItems, tabItems } from "./commands";
import { currentAccount } from "./app-bar";
import { needsYou } from "../../shared/needs-you";

export interface Command {
  /** Stable enough to remember as a recent pick. */
  key: string;
  /** The menu it's from (Pane, Tab, Places…), which its key starts with. */
  group: string;
  /** What it acts on or goes to, shown beside it: a plural noun (UI audit
   * #176). The group's, by KINDS, when not set. */
  kind?: string;
  /** It goes to a place itself (the swarm, chat, a page): the palette
   * leaves the place it's over alone. */
  place?: boolean;
  label: string;
  run: () => unknown;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  checked?: boolean;
  /** Where it is, for jumps: the session, the tab. */
  detail?: string;
  /** More words that find it, after its label (a jump's session and tab). */
  also?: string;
  /** The action it is, from its menu item: the same in two menus. */
  id?: string;
}

let open: { client: Client; phone: boolean } | null = null;
/** What had the keys before it opened: Escape gives them back (M111). */
let back: HTMLElement | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function openPalette(client: Client, phone = false) {
  countView("palette");
  closeMenu();
  if (!open) back = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  open = { client, phone };
  changed();
}

/** Closed without running anything: the focus goes back where it was (a
 * command run puts it on the pane, or on what it opens). */
function closePalette(restore = true) {
  open = null;
  changed();
  const to = back;
  back = null;
  if (restore && to?.isConnected) to.focus();
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
function fromMenu(group: string, items: MenuItem[], about?: string, heading: string | null = null, top = true): Command[] {
  const out: Command[] = [];
  let under: string | null = heading;
  let detail: string | undefined = about;
  items.forEach((item, i) => {
    if (typeof item === "object" && "submenu" in item) {
      // Listed as if it were in the menu, by their own names.
      out.push(...fromMenu(group, item.submenu, detail, under, false));
    } else if (item === "separator") under = null;
    else if ("header" in item) {
      if (top && i === 0) detail = item.header;
      else under = item.header;
    } else {
      const label = under ? `${under}: ${item.label}` : item.label;
      // A row that asks first in the menu asks here too, in a prompt.
      const run = item.confirm
        ? async () => {
            if ((await askText(`${item.label}? Type yes to confirm`, ""))?.trim().toLowerCase() === "yes") item.run();
          }
        : item.run;
      out.push({ key: `${group}/${label}`, group, detail, ...item, label, run });
    }
  });
  return out;
}

/** What each menu's commands are shown as: one pattern, plural nouns (UI
 * audit #176: PLACES, DOCS, PANE, GO TO). */
const KINDS: Record<string, string> = { Pane: "Panes", Tab: "Tabs", New: "Tabs", Session: "Sessions", Settings: "Settings", Account: "Account", Docs: "Docs", Places: "Places" };
export const kindOf = (c: Command) => c.kind ?? KINDS[c.group] ?? c.group;

/** Every command for what's shown now. */
export function commands(client: Client, phone: boolean, workspace: string | null, docs: DocRow[] = []): Command[] {
  const state = client.state;
  if (!state) return [];
  const out: Command[] = [];
  const pane = client.active();
  const tab = client.tabView();
  const session = client.session;

  // Which pane each acts on, beside it: "Pane" alone says nothing. Over a
  // place (the swarm, a page) that pane is hidden: say it's back on Panes
  // (UI audit #176).
  const under = here() === "panes" ? "" : " (on Panes)";
  if (pane !== undefined) out.push(...fromMenu("Pane", paneItems(client, pane, phone, workspace), `${client.title(pane) || client.info(pane)?.command || `pane %${pane}`}${under}`));
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

  // The account menu's settings: this machine's (always: UI audit #176,
  // "permission" and "developer" found nothing), this device's, help; and
  // the account's own (Teams, Devices…). Without the menus' headers: each
  // would name every item after it.
  const labelled = (items: MenuItem[]) => items.filter((i) => typeof i === "object" && "label" in i && i.label !== "Command palette…");
  out.push(...fromMenu("Settings", labelled(settingsItems(client, { machine: true }))));
  const account = currentAccount();
  if (account) out.push(...fromMenu("Account", labelled([...account.items(), { label: "Sign out", danger: true, run: () => account.signOut() }])));

  // Jumps: this machine's sessions, tabs, panes and workspaces by name
  // (M111: every pane, in any session).
  const toPane = (p: PaneId) => {
    client.setActive(p);
    requestAnimationFrame(() => client.viewOf(p)?.focus());
  };
  for (const s of state.sessions) {
    if (s.id !== session) out.push({ key: `Go/session ${s.id}`, group: "Go to", kind: "Sessions", label: `Go to session ${s.name}`, run: () => showSession(client, s.id) });
    for (const tid of s.tabs) {
      const t = client.tabView(tid);
      if (!t) continue;
      const label = tabLabel(client, t);
      const panes = paneIds(t);
      out.push({
        key: `Go/tab ${tid}`,
        group: "Go to",
        kind: "Tabs",
        label: `Go to tab ${label}`,
        detail: s.name,
        also: s.name,
        run: () => {
          client.selectTab(tid);
        },
      });
      for (const p of panes) {
        const info = client.info(p);
        if (!info) continue;
        if (info.type === "workspace") {
          const name = client.title(p).replace(/ \(chant\)$/, "");
          out.push({ key: `Go/pane ${p}`, group: "Go to", kind: "Panes", label: `Go to workspace ${name}`, detail: `${s.name} › ${label}`, run: () => toPane(p) });
        } else {
          const cwd = client.cwd(p)?.replace(/^\/(?:home|Users)\/[^/]+/, "~") ?? null;
          out.push({
            key: `Go/pane ${p}`,
            group: "Go to",
            kind: "Panes",
            label: `Go to pane ${paneName(client, info)}`,
            detail: [`${s.name} › ${label}`, cwd, info.branch].filter(Boolean).join(" · "),
            // Found by its session and tab too, and its directory's name.
            also: [s.name, label, cwd?.split("/").pop(), info.branch].filter(Boolean).join(" "),
            run: () => toPane(p),
          });
        }
      }
    }
  }

  // The Docs this machine keeps, by title (#1494).
  for (const d of docs) {
    out.push({
      key: `Docs/open ${d.id}`,
      group: "Docs",
      label: `Open Doc: ${d.title}`,
      detail: d.open === null || d.open === undefined ? undefined : "open now",
      run: async () => {
        const why = await openDoc(client, d.id);
        if (why) client.toast(why);
      },
    });
  }

  // The swarm, and the next pane that wants you after the active one.
  const next = nextNeedingYou(client);
  // "Go to Swarm" and "Go to Chat" are the places' (UI audit #176: each
  // was here twice).
  out.push({
    key: "Swarm/next",
    group: "Swarm",
    kind: "Panes",
    place: true,
    label: "Go to the next pane that needs you",
    shortcut: keyHint("pane.next-needs-you"),
    disabled: next === undefined,
    detail: next === undefined ? "nothing needs you" : client.title(next) || `pane %${next}`,
    run: () => next !== undefined && client.setActive(next),
  });
  out.push(...places(client, phone));
  out.push({
    key: "Swarm/next-in-swarm",
    group: "Swarm",
    kind: "Panes",
    place: true,
    label: "Show the next pane that needs you in the swarm",
    disabled: next === undefined,
    run: () => next !== undefined && (location.hash = `swarm=${next}`),
  });
  // A command two menus share once, by the action their items name (UI
  // audit #195: the pane's and the tab's Go to directory…, both the
  // active pane's; New tab, New VM tab), else by its menu and label.
  const seen = new Set<string>();
  return out.filter((c) => {
    const k = c.id ? `id/${c.id}` : c.key;
    return !seen.has(k) && !!seen.add(k);
  });
}

/** The next pane that needs you after the active one, wrapping round:
 * the palette's *Go to the next pane that needs you* and its key. */
export function nextNeedingYou(client: Client): PaneId | undefined {
  const pane = client.active();
  const wanting = (client.state?.panes ?? []).filter(needsYou).map((p) => p.id);
  return wanting.find((p) => p > (pane ?? -1)) ?? wanting[0];
}

/** Where the palette was opened: the panes, or a place over them. */
function here(): "panes" | "swarm" | "agents" | "docs" {
  return swarmRoute() ? "swarm" : agentsRoute() ? "agents" : docsRoute() ? "docs" : "panes";
}

/** The places, and what the one you're in offers: first when you're not
 * on the panes, so the palette starts with the view it's opened over. */
function places(client: Client, phone: boolean): Command[] {
  const at = here();
  const toPanes = () => {
    if (swarmRoute()) closeSwarm();
    if (agentsRoute()) closeAgentsPage();
    if (docsRoute()) closeDocsPage();
    closeOverview();
  };
  const out: Command[] = [];
  const go = (key: string, label: string, run: () => void, detail?: string) => out.push({ key: `Places/${key}`, group: "Places", place: true, label, detail, run });
  if (at !== "panes") go("panes", "Go to Panes", toPanes);
  // M111, on a computer: this machine's sessions at a glance, and what
  // waits on you on every machine.
  if (!phone) {
    if (!overviewRoute()) go("overview", "Go to the session overview", () => openOverview());
    go("attention", "Go to Needs you", () => openAttention(), "what waits on you, on every machine");
    // M113: a way out of a terminal (which keeps Tab) to the chrome, from
    // the keyboard; Tab and the arrows go on from there.
    if (at === "panes" && !overviewRoute()) {
      const focus = (key: string, label: string, sel: string, before?: () => void) =>
        go(key, label, () => {
          before?.();
          requestAnimationFrame(() => document.querySelector<HTMLElement>(sel)?.focus());
        });
      focus("focus-rail", "Move the keyboard to the rail", ".ws-rail button");
      go("focus-sessions", "Move the keyboard to the sessions (search)", focusSessionSearch);
      focus("focus-tabs", "Move the keyboard to the tabs", ".ws-tabs .tab[tabindex='0']");
      focus("focus-toolbar", "Move the keyboard to the pane toolbar", ".pane-toolbar button:not(:disabled)");
      focus("focus-header", "Move the keyboard to the header", ".ws-head button");
    }
  }
  if (at !== "swarm") go("swarm", "Go to Swarm", openSwarm, SWARM_ABOUT);
  // Chat is on the platform page (M153).
  if (hasChat()) {
    go("chat", "Go to Chat", () => openChat(), "every thread");
    go("activity", "Go to Chat: Activity", () => openChatView("activity"), "what needs you");
    go("threads", "Go to Chat: Threads you follow", () => openChatView("threads"));
  }
  if (client.flag("agent-delegation") && !client.state?.roles) {
    go("recipes", "Go to Agents: Recipes", () => openAgentsPage("recipes"));
    go("team", "Go to Agents: Team", () => openAgentsPage("team"));
  }
  if (!client.state?.roles) go("docs", "Go to Docs", openDocsPage);
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
      // Where a jump is (its session, directory, branch) finds it too,
      // after a match on its name.
      const d = a === null && b === null && c.also ? fuzzy(q, `${c.label} ${c.also}`) : null;
      const s = a === null ? (b ?? (d === null ? null : d - 2)) : b === null ? a : Math.max(a, b);
      return { c, s: s === null ? null : s + (at(c) < Infinity ? 4 : 0) - (c.disabled ? 20 : 0) };
    })
    .filter((x): x is { c: Command; s: number } => x.s !== null);
  scored.sort((a, b) => b.s - a.s);
  return scored.map((x) => x.c);
}

// ---------------------------------------------------------------- view

/** A command's icon: its kind's, and a place's own (UI audit #176: Docs
 * rows had the panes'). */
function iconOf(c: Command) {
  const k = kindOf(c);
  const to = c.group === "Places" ? c.key.slice("Places/".length) : "";
  if (k === "Settings" || k === "Account") return <SettingsIcon />;
  if (k === "Docs" || to === "docs") return <DocsIcon />;
  if (to === "swarm") return <SwarmIcon />;
  if (to === "chat" || to === "activity" || to === "threads") return <ChatIcon />;
  if (to === "recipes" || to === "team") return <AgentsIcon />;
  if (k === "Sessions" || k === "Places") return <SessionsIcon />;
  return <PanesIcon />;
}

function Palette({ client, phone, close }: { client: Client; phone: boolean; close: (restore?: boolean) => void }) {
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

  // The Docs this machine keeps, asked once when the palette opens (not on
  // every keystroke), for "Open Doc: TITLE".
  const [docs, setDocs] = useState<DocRow[]>([]);
  useEffect(() => {
    if (client.state?.roles) return;
    let live = true;
    client
      .request(OPS["docs.list"].method, opPath("docs.list"))
      .then((res) => (res.ok ? res.json<DocRow[]>() : []))
      .then((v) => live && Array.isArray(v) && setDocs(v))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [client]);

  const rows = useMemo(
    () => rank(commands(client, phone, isWorkspace ? cwd : null, docs), query, recent),
    [client, phone, isWorkspace, cwd, query, recent, client.state?.rev, pane, docs],
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
    close(false);
    // The panes are under the swarm and agents: show them, unless it's a
    // place's (it goes there itself).
    if (!c.place) {
      if (swarmRoute()) closeSwarm();
      if (agentsRoute()) closeAgentsPage();
      if (docsRoute()) closeDocsPage();
    }
    if (c.group !== "Places" && overviewRoute()) closeOverview();
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
    <div class="prompt-backdrop picker-backdrop" onPointerDown={(e) => e.target === e.currentTarget && close(false)}>
      {/* The dialog is the box itself, its input a combobox over the
          list (UI audit #176). */}
      <div class={phone ? "picker palette phone" : "picker palette"} role="dialog" aria-modal="true" aria-label="Command palette" onKeyDown={onKey}>
        <div class="palette-search"><SearchIcon size={20} /><input
          ref={input}
          class="picker-filter"
          placeholder="Type a command, a tab or a pane"
          role="combobox"
          aria-label="Command"
          aria-expanded="true"
          aria-autocomplete="list"
          aria-controls="palette-results"
          aria-activedescendant={rows[sel] ? `palette-result-${sel}` : undefined}
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
        </div><div class="palette-results-heading"><span>{query ? "Search results" : "Commands & destinations"}</span><span>{rows.length} available</span></div>
        <ul id="palette-results" class="picker-list" role="listbox" aria-label="Commands" ref={listRef}>
          {rows.length === 0 && <li class="picker-empty">Nothing matches</li>}
          {rows.map((c, i) => (
            <li
              key={c.key}
              id={`palette-result-${i}`}
              role="option"
              aria-selected={i === sel}
              aria-disabled={c.disabled}
              class={`picker-row palette-row${i === sel ? " selected" : ""}${c.disabled ? " disabled" : ""}${c.danger ? " danger" : ""}`}
              data-command={c.key}
              title={c.detail}
              onPointerMove={() => i !== sel && setSel(i)}
              onClick={() => run(c)}
            >
              <span class="palette-command-icon" aria-hidden="true">{iconOf(c)}</span>
              <span class="palette-command-text">
              <span class="picker-label">
                {c.label}
                {/* A switch that's on: a badge after it, not a dot glued
                    to its first word (UI audit #176). */}
                {c.checked && <span class="palette-badge">on</span>}
              </span>
              {c.detail && <span class="palette-detail">{c.detail}</span>}
              <span class="palette-group">{kindOf(c)}</span></span>
              {recent.includes(c.key) && !query && <span class="palette-recent">recent</span>}
              {c.shortcut && <kbd class="menu-key">{c.shortcut}</kbd>}
            </li>
          ))}
        </ul>
        <footer class="palette-help"><span><kbd>↑</kbd> <kbd>↓</kbd> Navigate</span><span><kbd>↵</kbd> Run command</span><span><kbd>Esc</kbd> Close</span></footer>
        {phone && (
          <div class="picker-actions">
            <button onClick={() => close()}>Cancel</button>
          </div>
        )}
      </div>
    </div>
  );
}
