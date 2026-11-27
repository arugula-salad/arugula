// A section of Settings: a display heading (with a count, where it lists
// things), one plain line saying what it is (its help), the rest of what
// there is to know under it, then what it holds. One shape for every
// section of every scope (Arugula #1104): an id (the address's), a title,
// a summary (a few words on what's set: "Weeknights 10 PM – 8 AM",
// "3 repos linked") and the detail, its children.
//
// Inside a scope's list (./settings-list.tsx, Lift 7) a section is a kit
// Row: its title and what's set on the line, its help and controls under
// it, open when the address names it. Closed, it stays drawn, hidden, so
// what it reads keeps its summary current. Anywhere else (a page that
// borrows one) it's drawn in place, under a heading.
//
// A section can hold sections (Arugula #1105): where a scope merges a few
// into one row ("Agents"), each it holds is a part of it, under a smaller
// heading, and tells it its summary. The row says the parts' summaries in
// a line, where the section says none of its own.

import { createContext, type ComponentChildren } from "preact";
import { useContext, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";
import { Row } from "../kit/row";

/** The list a section is in: the one open, and the way to open another. */
export interface SectionList {
  /** The section the address names: open, the rest closed. A section
   * that opens with something started (a form) when it's asked for reads it. */
  asked: string | null;
  /** Open this one, or close it where it's open. */
  toggle(id: string): void;
}

export const SectionListContext = createContext<SectionList | null>(null);

/** The section a part is in: where it says what it is. */
interface Whole {
  tell(id: string, summary: string | undefined, el: HTMLElement): void;
  forget(id: string, el: HTMLElement): void;
}

const WholeContext = createContext<Whole | null>(null);

/** Parts' summaries, in the page's order, in a line. */
function partsLine(parts: ReadonlyMap<string, { summary?: string; el: HTMLElement }>): string | undefined {
  const inOrder = [...parts.values()].sort((a, b) => (a.el.compareDocumentPosition(b.el) & Node.DOCUMENT_POSITION_FOLLOWING ? -1 : 1));
  return inOrder.map((p) => p.summary).filter(Boolean).join(" · ") || undefined;
}

export function Section({
  id,
  title,
  summary,
  count,
  lede,
  more,
  hooks,
  children,
}: {
  id: string;
  title: string;
  /** What's set, in a few words, for the list beside it. */
  summary?: string;
  /** How many it lists, beside the heading. */
  count?: number;
  /** Its help: the one plain line under the heading; left out where the heading says it. */
  lede?: ComponentChildren;
  /** The rest of what there is to know, under the help line. */
  more?: ComponentChildren;
  /** The section's own `data-*` hooks. */
  hooks?: Record<`data-${string}`, string>;
  children?: ComponentChildren;
}) {
  const list = useContext(SectionListContext);
  const whole = useContext(WholeContext);
  const el = useRef<HTMLElement>(null);
  // The parts it holds, and what each says.
  const [parts, setParts] = useState<ReadonlyMap<string, { summary?: string; el: HTMLElement }>>(new Map());
  const holds: Whole = useMemo(
    () => ({
      tell: (part, said, at) =>
        setParts((was) => {
          const o = was.get(part);
          return o && o.summary === said && o.el === at ? was : new Map(was).set(part, { summary: said, el: at });
        }),
      forget: (part, at) =>
        setParts((was) => {
          if (was.get(part)?.el !== at) return was;
          const next = new Map(was);
          next.delete(part);
          return next;
        }),
    }),
    [],
  );
  const said = summary ?? partsLine(parts);
  useLayoutEffect(() => {
    if (whole && el.current) whole.tell(id, said, el.current);
  }, [whole, id, said]);
  useLayoutEffect(() => {
    const at = el.current;
    return () => void (at && whole?.forget(id, at));
  }, [whole, id]);
  const body = (
    <>
      {lede ? <p class="settings-lede">{lede}</p> : null}
      {more ? <div class="settings-more">{more}</div> : null}
      <WholeContext.Provider value={holds}>{children}</WholeContext.Provider>
    </>
  );
  // A section of a scope's list: a row, open at its address. Its body
  // holds the section's hooks, so they're seen only while it's open.
  if (list && !whole)
    return (
      <Row
        id={id}
        title={
          <>
            {title}
            {count !== undefined ? <span class="m-meta"> {count}</span> : null}
          </>
        }
        meta={said ? [said] : undefined}
        open={list.asked === id}
        keep
        onClick={() => list.toggle(id)}
        data-settings-row={id}
        data-settings-summary={said}
      >
        <div class="settings-sec-body" data-settings-section={id} data-settings-title={title} data-settings-summary={said} {...hooks}>
          {body}
        </div>
      </Row>
    );
  const Heading = whole ? "h3" : "h2";
  return (
    <section
      ref={el}
      class={whole ? "settings-part" : "settings-sec"}
      id={id}
      aria-labelledby={`${id}-h`}
      {...(whole ? { "data-settings-part": id } : { "data-settings-section": id, "data-settings-title": title })}
      data-settings-summary={said}
      {...hooks}
    >
      <Heading id={`${id}-h`}>
        {title}
        {count !== undefined ? <span class="dim">{count}</span> : null}
      </Heading>
      {body}
    </section>
  );
}
