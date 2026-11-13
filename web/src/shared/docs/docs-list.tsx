// The one Docs list (Arugula #864): the machine's Docs page (shell/ui/docs-page.tsx),
// the platform's (platform/pages/docs.tsx) and the phone all draw this, on
// the list page. Its two modes are never mixed (./docs-mode.ts): a person's
// own Docs, titled My Docs with no tabs, or a team's, in two tabs, My Docs
// and Team Docs, the one used last remembered.
//
// What it draws is the same wherever it is: the rows in time groups by the
// sort's time (#825), each with its layout mark (#829, #830), its shape and
// its time, a search over titles, openings and headings, and the reader in
// the preview. Where a Doc is open, its actions (#820), the selection (#818)
// and what a row's ⋯ holds are the host's, since a library Doc and a
// project's are reached differently.
//
// A row's mark is the Doc's shape: a segment a section, as long as the section
// is, brighter where it has a figure, and the whole as long as the Doc is. So a
// note and a long plan look different without opening either.

import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import { EmptyLine } from "../ui/block-states";
import { ListPage, type ListGroup, type ListRowParts, type ListTicked } from "../ui/list-page";
import { PageHeader, type BackLink } from "../ui/page-header";
import { DocRowTime, DOC_SORTS, sortTime, useDocSort } from "./doc-sort";
import type { DocTimesOf } from "./doc-times";
import { LayoutMark } from "./layout-mark";
import { inTab, savedTab, saveTab, type DocsMode, type DocsTab } from "./docs-mode";
import "./docs.css";

/** A section as the shape and the outline draw it. */
export interface DocShapeSection {
  heading: string;
  words: number;
  figures: number;
}

/** A Doc as the list draws it, whichever host's it is. */
export interface DocsListItem extends DocTimesOf {
  id: string;
  title: string;
  layout?: string;
  updated: number;
  /** Its sections, once they're known. */
  sections?: DocShapeSection[];
  /** Its opening, for the search. */
  summary?: string;
  /** Made by the reader or their agents: what My Docs shows in a team. */
  mine: boolean;
}

/** A Doc as long as this is as long as a mark gets. */
const LONG_DOC = 20_000;
const MARK_W = 130;

/** The Doc's shape: see the top of this file. */
export function Shape({ sections }: { sections: DocShapeSection[] }) {
  if (!sections.length) return null;
  const words = sections.reduce((n, s) => n + s.words, 0);
  const width = Math.max(26, Math.sqrt(Math.min(words, LONG_DOC) / LONG_DOC) * MARK_W);
  return (
    <span class="doc-shape" style={{ width: `${Math.round(width)}px` }} aria-hidden="true" data-doc-shape>
      {sections.map((s, i) => (
        <i key={i} class={s.figures ? "fig" : undefined} style={{ flex: `${Math.max(s.words, words * 0.012, 1)} 0 0` }} />
      ))}
    </span>
  );
}

/** The preview's outline: each heading, which scrolls the reader to it, with
 * a bar as long as its section. */
export function Outline({ sections, go }: { sections: DocShapeSection[]; go: (i: number) => void }) {
  if (!sections.length) return null;
  const biggest = Math.max(1, ...sections.map((s) => s.words));
  return (
    <>
      <h3>Outline</h3>
      <ol class="doc-outline" data-doc-outline>
        {sections.map((s, i) => (
          <li key={i}>
            <button type="button" class="link doc-outline-go" title={s.heading} data-doc-outline-go onClick={() => go(i)}>
              {s.heading}
            </button>
            <i class={s.figures ? "fig" : undefined} style={{ width: `${Math.max(6, (s.words / biggest) * 100)}%` }} />
          </li>
        ))}
      </ol>
    </>
  );
}

/** Scroll the reader in `root` to its `i`th section. */
export const goToSection = (root: HTMLElement | null, i: number) => root?.querySelectorAll("[data-section]")[i]?.scrollIntoView({ block: "start" });

/** Which of the list's groups a Doc falls in, by the day of the time it's sorted by. */
export function ageGroup(at: number, now: number): string {
  const day = new Date(now);
  day.setHours(0, 0, 0, 0);
  if (at >= day.getTime()) return "Today";
  day.setDate(day.getDate() - 1);
  if (at >= day.getTime()) return "Yesterday";
  day.setDate(day.getDate() - 5);
  if (at >= day.getTime()) return "This week";
  return "Earlier";
}

const TABS: [DocsTab, string][] = [
  ["mine", "My Docs"],
  ["team", "Team Docs"],
];

/** The tab shown in a team, remembered in this browser. */
function useTab(): [DocsTab, (t: DocsTab) => void] {
  const [tab, setTab] = useState<DocsTab>(() => savedTab());
  return [
    tab,
    (t) => {
      setTab(t);
      saveTab(t);
    },
  ];
}

export interface DocsListProps<T> {
  mode: DocsMode;
  /** Every Doc there is to show, `null` while they load. */
  docs: T[] | null;
  item: (d: T) => DocsListItem;
  splitKey: string;
  /** Under the title. */
  sub?: ComponentChildren;
  /** In place of the mode's title ("My Docs", "Docs"), and the way back to another view. */
  title?: string;
  back?: BackLink;
  /** At the title's right: New Doc. */
  actions?: ComponentChildren;
  /** The counts, of the Docs the tab has (none, while they load). */
  stats?: (inTab: T[]) => ComponentChildren;
  /** The host's own narrowing (the machine's Open and Shared), and what it says when it leaves none. */
  narrow?: { test: (d: T) => boolean; none: string };
  /** Under the header: the Trash. */
  under?: ComponentChildren;
  /** In the list's place with none at all. */
  empty: ComponentChildren;
  /** Said while they load: nothing, once loading failed and `notice` says why. */
  loading: ComponentChildren;
  notice?: ComponentChildren;
  /** The host's parts of a row: its tags, its ⋯, its hooks. */
  row?: (d: T) => Partial<ListRowParts>;
  preview: (d: T) => ComponentChildren;
  onOpen: (d: T) => void;
  /** Whether the tab shown takes a selection, and the bar that acts on it. */
  selectable?: (tab: DocsTab | null) => boolean;
  selection?: (ticked: ListTicked<T>) => ComponentChildren;
}

export function DocsList<T>({ mode, docs, item, splitKey, sub, title, back, actions, stats, narrow, under, empty, loading, notice, row, preview, onOpen, selectable, selection }: DocsListProps<T>) {
  const [saved, setTab] = useTab();
  const tab = mode.kind === "team" ? saved : null;
  const [query, setQuery] = useState("");
  const [sort, setSort] = useDocSort();
  const all = docs ?? [];
  const tabbed = all.filter((d) => tab === null || inTab(tab, item(d).mine));
  const q = query.trim().toLowerCase();
  const shown = tabbed.filter((d) => {
    if (narrow && !narrow.test(d)) return false;
    if (!q) return true;
    const it = item(d);
    return it.title.toLowerCase().includes(q) || !!it.summary?.toLowerCase().includes(q) || !!it.sections?.some((s) => s.heading.toLowerCase().includes(q));
  });
  const now = Date.now();
  const groups: ListGroup<T>[] = [];
  const at = (d: T) => sortTime(item(d), sort);
  for (const d of [...shown].sort((a, b) => at(b) - at(a))) {
    const title = ageGroup(at(d), now);
    const last = groups[groups.length - 1];
    if (last?.title === title) last.items.push(d);
    else groups.push({ title, items: [d] });
  }

  const status =
    docs === null ? (
      loading
    ) : all.length === 0 ? (
      empty
    ) : tabbed.length === 0 ? (
      <EmptyLine data-docs-none>{`None of ${mode.kind === "team" ? mode.name : "the team"}'s Docs is yours or your agents' yet.`}</EmptyLine>
    ) : shown.length === 0 ? (
      <EmptyLine data-docs-none>{q ? "No Doc's title, opening or headings match." : (narrow?.none ?? "No Doc is here.")}</EmptyLine>
    ) : undefined;

  // ← and → go to the other tab, as a tablist's keys do.
  const other = (t: DocsTab): DocsTab => (t === "mine" ? "team" : "mine");
  const tabs = tab !== null && (
    <div class="seg docs-tabs" role="tablist" aria-label="Docs">
      {TABS.map(([t, label]) => (
        <button
          key={t}
          type="button"
          role="tab"
          id={`docs-tab-${t}`}
          aria-selected={tab === t}
          tabIndex={tab === t ? 0 : -1}
          data-docs-tab={t}
          onClick={() => setTab(t)}
          onKeyDown={(e) => {
            if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
            setTab(other(t));
            document.getElementById(`docs-tab-${other(t)}`)?.focus();
          }}
        >
          {label}
        </button>
      ))}
    </div>
  );

  return (
    <ListPage<T>
      label="Docs"
      splitKey={splitKey}
      header={
        <>
          <PageHeader title={title ?? (tab === null ? "My Docs" : "Docs")} back={back} sub={sub} actions={actions} stats={stats?.(tabbed)}>
            {tabs}
          </PageHeader>
          {under}
        </>
      }
      searchLabel="Search titles and headings"
      query={query}
      setQuery={setQuery}
      sort={{ options: DOC_SORTS, value: sort, onSort: setSort }}
      groups={groups}
      keyOf={(d) => item(d).id}
      previewOn="click"
      onOpen={onOpen}
      notice={notice}
      status={status && <div class="list-status">{status}</div>}
      selectable={selectable?.(tab) ?? false}
      selection={selection}
      row={(d) => {
        const it = item(d);
        const own = row?.(d) ?? {};
        return {
          dense: true,
          name: it.title,
          title: (
            <>
              <LayoutMark layout={it.layout} />
              <span data-doc-title>{it.title}</span>
            </>
          ),
          age: <DocRowTime doc={it} by={sort} />,
          mark: it.sections && <Shape sections={it.sections} />,
          ...own,
          attrs: { "data-doc": it.id, "data-mine": String(it.mine), ...own.attrs },
        };
      }}
      preview={preview}
    />
  );
}
