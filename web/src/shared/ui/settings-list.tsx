// Settings as a list and its detail (Arugula #1104, layout B): on the left
// a scope's sections, each a row with its title and what's set; on the
// right the chosen one's controls, with its help under its heading. The
// List pages' look (./list-page.css, as Docs has it), but rows are links:
// the chosen section is the address's, so a link opens it.
//
// Every section of the scope is drawn (./settings-section.tsx), the chosen
// one shown and the rest hidden, so each keeps reading what it reads and
// its row's summary stays current. A section tells the list its title and
// summary as it's drawn, and the rows follow the page's order. One that
// draws nothing (Claude, until there's something in it) has no row.
//
// Too narrow for both (a phone, by the room the page has): the list alone,
// and a row opens its section full width, with a way back to the list.

import type { ComponentChildren } from "preact";
import { useCallback, useEffect, useMemo, useRef, useState } from "preact/hooks";
import { Card } from "./card";
import { ChevronIcon } from "./icons";
import { SectionListContext, type ListedSection, type SectionList } from "./settings-section";
import "./list-page.css";

/** Under this, the list stands alone (as in ./list-page.css). */
const NARROW = 760;

/** The rows in the page's order. */
const inOrder = (secs: Iterable<ListedSection>) => [...secs].sort((a, b) => (a.el.compareDocumentPosition(b.el) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1));

export function SettingsList({
  label,
  header,
  section,
  hrefOf,
  open,
  back,
  children,
  ...attrs
}: {
  /** What the list is, for assistive tech: "Settings". */
  label: string;
  /** Over the rows: the page's header, its scopes and picker. */
  header: ComponentChildren;
  /** The section the address names, if it names one. */
  section?: string;
  /** A section's address; `null`, the list's own (no section chosen). */
  hrefOf: (section: string | null) => string;
  /** Going to an address without a reload. */
  open: (e: MouseEvent, href: string) => void;
  /** The way back to the list where the detail stands alone: its name. */
  back: string;
  /** The scope's sections. */
  children: ComponentChildren;
  [hook: `data-${string}`]: string | undefined;
}) {
  const [secs, setSecs] = useState<ReadonlyMap<string, ListedSection>>(new Map());
  const tell = useCallback((s: ListedSection) => {
    setSecs((was) => {
      const o = was.get(s.id);
      if (o && o.title === s.title && o.summary === s.summary && o.el === s.el) return was;
      return new Map(was).set(s.id, s);
    });
  }, []);
  const forget = useCallback((id: string, el: HTMLElement) => {
    setSecs((was) => {
      if (was.get(id)?.el !== el) return was;
      const next = new Map(was);
      next.delete(id);
      return next;
    });
  }, []);
  const rows = useMemo(() => inOrder(secs.values()), [secs]);
  // The address's section, else the first: a wide page always shows one.
  const shown = section && secs.has(section) ? section : (rows[0]?.id ?? null);
  const list: SectionList = useMemo(() => ({ shown, asked: section ?? null, tell, forget }), [shown, section, tell, forget]);
  // Too narrow for both, by the room the page has: with no section
  // chosen, nothing is shown beside the list, so no row is marked.
  const root = useRef<HTMLDivElement>(null);
  const [narrow, setNarrow] = useState(false);
  useEffect(() => {
    const el = root.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const seen = new ResizeObserver(() => setNarrow(el.clientWidth <= NARROW));
    seen.observe(el);
    return () => seen.disconnect();
  }, []);
  const marked = section || !narrow ? shown : null;
  // A new section starts at its top.
  const detail = useRef<HTMLDivElement>(null);
  useEffect(() => detail.current?.scrollTo?.({ top: 0 }), [shown]);

  const shownRow = rows.find((r) => r.id === shown);
  return (
    <div ref={root} class={section ? "list-page settings-list detail" : "list-page settings-list"} data-list-page data-settings-list {...attrs}>
      <div class="list-split">
        <section class="list-pane" aria-label={label}>
          <div class="list-head">{header}</div>
          <div class="list-scroll">
            <nav class="list-rows" aria-label={`${label}: sections`}>
              {rows.map((r) => {
                const href = hrefOf(r.id);
                return (
                  <Card as="a" key={r.id} class="list-row" href={href} selected={r.id === marked} data-settings-row={r.id} onClick={(e: MouseEvent) => open(e, href)}>
                    <div class="list-row-top">
                      <span class="list-row-title">{r.title}</span>
                    </div>
                    <div class="list-row-detail" title={r.summary}>
                      {r.summary ?? " "}
                    </div>
                  </Card>
                );
              })}
            </nav>
          </div>
        </section>
        <aside class="list-preview settings-detail" aria-label={shownRow?.title ?? label} data-settings-detail={shown ?? ""}>
          <a class="link list-back" href={hrefOf(null)} data-settings-back onClick={(e) => open(e, hrefOf(null))}>
            <ChevronIcon dir="left" /> {back}
          </a>
          <div class="settings-detail-body sw-page" ref={detail}>
            <SectionListContext.Provider value={list}>{children}</SectionListContext.Provider>
          </div>
        </aside>
      </div>
    </div>
  );
}
