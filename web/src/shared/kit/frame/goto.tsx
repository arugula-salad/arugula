// Go to… (Lift 3, #1479; v6's): one overlay, from anywhere (⌘K, `g p`).
// Empty, it lists the places you went to last, across projects; typing
// matches projects, boards, cards, Docs, agents and a machine's tabs and
// panes together, by the start of their words, and the commands rank under
// them. ↵ goes, ↑ ↓ move, Esc closes. (Not v6's j k: a name starting with
// j or k couldn't be typed.)
//
// It knows nothing of either page: each entry adds a source (the app's
// projects, boards and Docs; a machine's sessions, tabs, panes and
// commands), read each time it opens and on each keystroke. A source may
// also offer a search of its own (a machine's output), as a mode.

import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { fuzzy } from "../../fuzzy";
import { both, keyHint, os, typingIn, useBinding } from "../../keys";
import { useSubscribe } from "../../ui/hooks";
import { Kbd } from "../kbd";
import { Sheet } from "../sheet";
import { Mark, type State } from "../state-mark";
import "../kit.css";

export interface GoItem {
  /** Stable enough to remember as a recent pick. */
  key: string;
  /** What it is, a word: board, card, doc, tab, pane, project, command. */
  kind: string;
  /** The project it's in, first and bold. */
  project?: string;
  title: string;
  /** Where it runs, dim and mono at the right. */
  machine?: string;
  state?: State;
  /** More words that find it (a pane's directory, a tab's session). */
  also?: string;
  /** A command: ranked under the places. */
  command?: boolean;
  /** A command's key, from the key table. */
  hint?: string;
  disabled?: boolean;
  danger?: boolean;
  /** "7 for you", in the projects list. */
  note?: string;
  /** A place you were lately (the back stack's), newest first: first when
   * nothing's typed. */
  recent?: boolean;
  run: () => unknown;
}

export type Scope = "all" | "projects";

/** A search a source runs itself (a machine's output), as Go to…'s mode. */
export interface GoSearch {
  id: string;
  /** The field's name ("Search output"). */
  label: string;
  placeholder: string;
  /** What the list says while it's empty, by what's typed. */
  empty: (q: string, busy: boolean) => string;
  search: (q: string) => Promise<{ items: GoItem[]; note?: string }>;
}

export interface GoSource {
  items: (scope: Scope) => GoItem[];
  /** The rows for what's typed that no list has (Search output for "…"). */
  typed?: (q: string) => GoItem[];
  searches?: GoSearch[];
  subscribe?: (fn: () => void) => () => void;
}

const sources = new Set<GoSource>();
let open: { scope: Scope; mode: string | null; query: string } | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());

export function openGoTo(scope: Scope = "all", query = "", mode: string | null = null) {
  open = { scope, mode, query };
  changed();
}
export const closeGoTo = () => {
  open = null;
  changed();
};
export const goToOpen = () => !!open;

/** Add a source while the page is up; the answer takes it away. */
export function addGoSource(s: GoSource): () => void {
  sources.add(s);
  return () => void sources.delete(s);
}

const RECENT_KEY = "arugula.goto.recent";
const RECENT_MAX = 9;

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
    localStorage.setItem(RECENT_KEY, JSON.stringify([key, ...recents().filter((k) => k !== key)].slice(0, RECENT_MAX * 3)));
  } catch {
    // No storage (a private window): no recents.
  }
}

/** By how well the words' starts match, a command a little under a place
 * that matches as well; with nothing typed, recent picks, places, commands. */
export function rank(list: GoItem[], query: string, recent: string[]): GoItem[] {
  const q = query.trim();
  const at = (c: GoItem) => {
    const i = recent.indexOf(c.key);
    return i < 0 ? Infinity : i;
  };
  if (!q) {
    const lately = list.filter((c) => c.recent);
    const first = list.filter((c) => !c.recent && at(c) < Infinity).sort((a, b) => at(a) - at(b));
    const rest = list.filter((c) => !c.recent && at(c) === Infinity);
    return [...lately, ...first, ...rest.filter((c) => !c.command), ...rest.filter((c) => c.command)];
  }
  const scored = list
    .map((c) => {
      const named = c.project ? `${c.project} ${c.title}` : c.title;
      const a = fuzzy(q, c.title);
      const b = fuzzy(q, named);
      const d = a === null && b === null && c.also ? fuzzy(q, `${named} ${c.also}`) : null;
      const s = a === null ? (b ?? (d === null ? null : d - 2)) : b === null ? a : Math.max(a, b);
      return { c, s: s === null ? null : s + (at(c) < Infinity || c.recent ? 4 : 0) - (c.disabled ? 20 : 0) - (c.command ? 6 : 0) };
    })
    .filter((x): x is { c: GoItem; s: number } => x.s !== null);
  scored.sort((a, b) => b.s - a.s);
  return scored.map((x) => x.c);
}

/** ⌘K from anywhere on a Mac; elsewhere Ctrl+K, and Ctrl+Shift+K from a
 * terminal or a field (`when`). */
const withK = (mods: string[]) => mods.map((m) => `${m}+KeyK`);
const CHORDS = { mac: withK(["Meta", "Ctrl+Shift"]), other: withK(["Ctrl", "Ctrl+Shift"]) };

/** Go to…, and its keys; once per page. */
export function GoToLayer() {
  useSubscribe((fn) => {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  });
  // ⌘K from anywhere on a Mac; elsewhere Ctrl+K leaves a terminal and a
  // field their kill-line, and Ctrl+Shift+K is the one from anywhere.
  useBinding(
    {
      id: "goto",
      label: "Go to…",
      group: "go",
      chip: "go to",
      keys: CHORDS,
      when: (e) => os === "mac" || e.shiftKey || !typingIn(document.activeElement),
      run: () => (open ? closeGoTo() : openGoTo()),
    },
    [],
  );
  useBinding({ id: "goto.g", label: "Go to…", group: "go", keys: both("key:g key:p"), run: () => openGoTo() }, []);
  if (!open) return null;
  return <GoTo key={`${open.scope}/${open.mode}`} scope={open.scope} mode={open.mode} initial={open.query} />;
}

function GoTo({ scope, mode, initial }: { scope: Scope; mode: string | null; initial: string }) {
  const [query, setQuery] = useState(initial);
  const [sel, setSel] = useState(0);
  const [recent] = useState(recents);
  // Something ran: it put the focus where it goes, so shutting doesn't.
  const ran = useRef(false);
  const list = useRef<HTMLUListElement>(null);
  const [, tick] = useState(0);
  useEffect(() => {
    const offs = [...sources].map((s) => s.subscribe?.(() => tick((n) => n + 1)));
    return () => offs.forEach((off) => off?.());
  }, []);
  const search = mode ? [...sources].flatMap((s) => s.searches ?? []).find((x) => x.id === mode) : undefined;
  const found = useSearch(search, query);
  const rows = useMemo(() => {
    if (search) return found.items;
    const all = [...sources].flatMap((s) => s.items(scope));
    const ranked = rank(all, query, recent);
    return query.trim() && scope === "all" ? [...ranked, ...[...sources].flatMap((s) => s.typed?.(query.trim()) ?? [])] : ranked;
  }, [query, found, scope]);
  useEffect(() => setSel((s) => Math.min(s, Math.max(0, rows.length - 1))), [rows.length]);
  useEffect(() => list.current?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: "nearest" }), [sel, rows]);

  const go = (c: GoItem | undefined) => {
    if (!c || c.disabled) return;
    if (!search) remember(c.key);
    ran.current = true;
    closeGoTo();
    void c.run();
  };
  const move = (by: number) => setSel((s) => Math.max(0, Math.min(rows.length - 1, s + by)));
  const onKey = (e: KeyboardEvent) => {
    if (e.key === "ArrowDown") (e.preventDefault(), move(1));
    else if (e.key === "ArrowUp") (e.preventDefault(), move(-1));
    else if (e.key === "Enter") (e.preventDefault(), go(rows[sel]));
  };
  const heading = search ? (found.busy ? "Searching…" : (found.note ?? "")) : query.trim() ? "" : scope === "projects" ? "Projects" : "Recent, every project";
  const hint = keyHint("goto");
  return (
    <Sheet title={scope === "projects" ? "Switch project" : "Go to…"} titled={false} returnFocus={() => !ran.current} onClose={closeGoTo}>
      <div class="k-goto" data-goto={scope} data-goto-mode={mode ?? undefined}>
        <label class="k-goto-field">
          <span aria-hidden="true">⌕</span>
          {scope === "projects" && <span class="k-goto-scope m-meta">projects</span>}
          <input
            data-autofocus
            role="combobox"
            aria-label={search ? search.label : "Go to"}
            aria-expanded="true"
            aria-controls="goto-results"
            aria-activedescendant={rows[sel] ? `goto-${sel}` : undefined}
            placeholder={search ? search.placeholder : scope === "projects" ? "Switch project…" : "Go to a project, board, card, Doc, agent or tab…"}
            value={query}
            autocomplete="off"
            spellcheck={false}
            onInput={(e) => (setQuery(e.currentTarget.value), setSel(0))}
            onKeyDown={onKey}
          />
          <Kbd keys="esc" />
        </label>
        {heading && (
          <p class="k-goto-heading m-meta" data-goto-note>
            {heading}
          </p>
        )}
        <ul id="goto-results" class="k-goto-list" role="listbox" aria-label={search ? "Found" : "Places and commands"} ref={list}>
          {rows.length === 0 && <li class="k-goto-empty m-meta">{search ? search.empty(query.trim(), found.busy) : "Nothing matches"}</li>}
          {rows.map((c, i) => (
            <li
              key={c.key}
              id={`goto-${i}`}
              role="option"
              aria-selected={i === sel}
              aria-disabled={c.disabled}
              class={`k-goto-item${c.danger ? " is-danger" : ""}`}
              data-command={c.key}
              onPointerMove={() => i !== sel && setSel(i)}
              onClick={() => go(c)}
            >
              {c.state ? <Mark state={c.state} /> : <span class="k-state" aria-hidden="true" />}
              <span class="k-goto-title">
                {c.project && (
                  <>
                    <b>{c.project}</b>
                    <span class="k-goto-dot" aria-hidden="true">
                      ·
                    </span>
                  </>
                )}
                {c.title}
              </span>
              {c.note && <span class="m-meta">{c.note}</span>}
              <span class="k-goto-kind m-meta">{c.kind}</span>
              {c.hint ? <Kbd keys={c.hint} /> : <span class="k-goto-machine m-id">{c.machine}</span>}
            </li>
          ))}
        </ul>
        <p class="k-goto-foot m-meta">
          <span>
            <Kbd keys="↵" /> go
          </span>
          <span>
            <Kbd keys="↑ ↓" /> move
          </span>
          {hint && (
            <span>
              <Kbd keys={hint} /> close
            </span>
          )}
        </p>
      </div>
    </Sheet>
  );
}

/** A mode's search: what's typed, a moment after typing stops; only the
 * newest answer is shown. */
function useSearch(search: GoSearch | undefined, query: string) {
  const [found, setFound] = useState<{ items: GoItem[]; note?: string; busy: boolean }>({ items: [], busy: false });
  useEffect(() => {
    const q = query.trim();
    if (!search || q.length < 2) return setFound({ items: [], busy: false });
    let live = true;
    setFound((f) => ({ ...f, busy: true }));
    const t = window.setTimeout(() => void search.search(q).then((r) => live && setFound({ ...r, busy: false })), 250);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [search, query]);
  return found;
}
