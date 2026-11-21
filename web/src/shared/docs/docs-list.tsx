// The one Docs list (Arugula #864): the machine's Docs page (shell/ui/docs-page.tsx),
// the platform's (platform/pages/docs.tsx) and the phone all draw this, on
// the list page. Its two modes are never mixed (./docs-mode.ts): a person's
// own Docs, titled My Docs with no tabs, or a team's, in two tabs, My Docs
// and Team Docs, the one used last remembered.
//
// What it draws is the same wherever it is: the rows in time groups by the
// sort's time (#825), each with its layout mark (#829, #830) and its time
// and nothing else (#1134), a search over titles, openings and headings, and
// the reader in the preview, its facts saying how long it is. Where a Doc is
// open, its actions (#820), the selection (#818) and what a row's ⋯ holds are
// the host's, since a library Doc and a project's are reached differently. A
// host with deleted Docs (a team's, Arugula #1133) has them in a folded
// Deleted group at the list's foot, there only when there are any.

import type { ComponentChildren } from "preact";
import type { RefObject } from "preact";
import { useEffect, useState } from "preact/hooks";
import { EmptyLine } from "../ui/block-states";
import { ListPage, type ListGroup, type ListRowParts, type ListTicked } from "../ui/list-page";
import { PageHeader, type BackLink } from "../ui/page-header";
import { DocRowTime, DOC_SORTS, sortTime, useDocSort } from "./doc-sort";
import type { DocTimesOf } from "./doc-times";
import { LayoutMark } from "./layout-mark";
import { inTab, savedTab, saveTab, type DocsMode, type DocsTab } from "./docs-mode";
import "./docs.css";

/** A section as the facts and the outline count it. */
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

const plural = (n: number, one: string) => `${n} ${one}${n === 1 ? "" : "s"}`;

/** Words a minute, for a Doc's reading time. */
const READ_WPM = 230;

/** A Doc's facts line (#1134): "8 sections · 12 min read". */
export function docFacts(sections: DocShapeSection[]): string | null {
  if (!sections.length) return null;
  const words = sections.reduce((n, s) => n + s.words, 0);
  return [plural(sections.length, "section"), words > 0 && `${Math.max(1, Math.round(words / READ_WPM))} min read`].filter(Boolean).join(" · ");
}

/** Scroll the reader in `root` to its `i`th section. */
const goToSection = (root: HTMLElement | null, i: number) => root?.querySelectorAll("[data-section]")[i]?.scrollIntoView({ block: "start" });

/** How far under the top of the reader's scroll a section's top may be and
 * still be the one being read. */
const READING_EDGE = 48;

/** The nearest box around `el` that scrolls. */
function scrollerOf(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const y = getComputedStyle(p).overflowY;
    if (y === "auto" || y === "scroll") return p;
  }
  return null;
}

/** Which section of the reader in `reader` is being read: the last one whose
 * top has reached the top of the scroll, or the last one once it's scrolled
 * to the end. */
function useReading(reader: RefObject<HTMLElement | null>, count: number): number {
  const [at, setAt] = useState(0);
  useEffect(() => {
    const root = reader.current;
    const box = root && scrollerOf(root);
    if (!root || !box) return;
    let frame = 0;
    const look = () => {
      frame = 0;
      const all = root.querySelectorAll("[data-section]");
      const edge = box.getBoundingClientRect().top + READING_EDGE;
      let i = 0;
      all.forEach((s, n) => {
        if (s.getBoundingClientRect().top <= edge) i = n;
      });
      if (box.scrollTop > 0 && box.scrollTop + box.clientHeight >= box.scrollHeight - 2) i = all.length - 1;
      setAt(Math.max(0, Math.min(i, count - 1)));
    };
    const soon = () => {
      if (!frame) frame = requestAnimationFrame(look);
    };
    look();
    box.addEventListener("scroll", soon, { passive: true });
    return () => {
      box.removeEventListener("scroll", soon);
      cancelAnimationFrame(frame);
    };
  }, [reader.current, count]);
  return at;
}

/** The preview's outline (#1135): each heading, which scrolls the reader to
 * it, the one being read marked as it scrolls, and ◧ on a section that holds
 * a figure (a chart, svg, prototype or picture). */
export function Outline({ sections, reader }: { sections: DocShapeSection[]; reader: RefObject<HTMLElement | null> }) {
  const at = useReading(reader, sections.length);
  if (!sections.length) return null;
  return (
    <>
      <h3>Outline</h3>
      <ol class="doc-outline" data-doc-outline>
        {sections.map((s, i) => (
          <li key={i} data-doc-outline-at={i === at ? "" : undefined}>
            <button type="button" class="link doc-outline-go" title={s.heading} aria-current={i === at ? "location" : undefined} data-doc-outline-go onClick={() => goToSection(reader.current, i)}>
              {s.heading}
            </button>
            {s.figures > 0 && (
              <span class="doc-outline-fig" title="Holds a figure" data-doc-outline-fig>
                ◧
              </span>
            )}
          </li>
        ))}
      </ol>
    </>
  );
}

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
  /** Under the header: the cloud line, New Doc's dialog. */
  under?: ComponentChildren;
  /** The deleted Docs, the latest deleted first: a folded Deleted group at the foot, when there are any. */
  deleted?: T[];
  /** After the rows, scrolling with them: the links the reader shared (Arugula #1315). */
  below?: ComponentChildren;
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

export function DocsList<T>({ mode, docs, item, splitKey, sub, title, back, actions, stats, narrow, under, deleted, below, empty, loading, notice, row, preview, onOpen, selectable, selection }: DocsListProps<T>) {
  const [saved, setTab] = useTab();
  const tab = mode.kind === "team" ? saved : null;
  const [query, setQuery] = useState("");
  const [sort, setSort] = useDocSort();
  const [unfolded, setUnfolded] = useState(false);
  const all = docs ?? [];
  const inThisTab = (d: T) => tab === null || inTab(tab, item(d).mine);
  const tabbed = all.filter(inThisTab);
  const q = query.trim().toLowerCase();
  const found = (d: T) => {
    if (!q) return true;
    const it = item(d);
    return it.title.toLowerCase().includes(q) || !!it.summary?.toLowerCase().includes(q) || !!it.sections?.some((s) => s.heading.toLowerCase().includes(q));
  };
  const shown = tabbed.filter((d) => (!narrow || narrow.test(d)) && found(d));
  const now = Date.now();
  const groups: ListGroup<T>[] = [];
  const at = (d: T) => sortTime(item(d), sort);
  for (const d of [...shown].sort((a, b) => at(b) - at(a))) {
    const title = ageGroup(at(d), now);
    const last = groups[groups.length - 1];
    if (last?.title === title) last.items.push(d);
    else groups.push({ title, items: [d] });
  }
  const gone = (deleted ?? []).filter((d) => inThisTab(d) && found(d));
  if (gone.length > 0) groups.push({ title: "Deleted", key: "deleted", items: gone, fold: { open: unfolded, toggle: () => setUnfolded(!unfolded) }, attrs: { "data-docs-deleted": "" } });

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
      below={below}
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
          ...own,
          attrs: { "data-doc": it.id, "data-mine": String(it.mine), ...own.attrs },
        };
      }}
      preview={preview}
    />
  );
}
