// A section of Settings: a display heading (with a count, where it lists
// things), one plain line saying what it is (its help), the rest of what
// there is to know under it, then what it holds. One shape for every
// section of every scope (Arugula #1104): an id (the address's), a title,
// a summary (a few words on what's set: "Weeknights 10 PM – 8 AM",
// "3 repos linked") and the detail, its children.
//
// Inside a scope's list (./settings-list.tsx) a section tells the list its
// title and summary, so the list can say them, and it shows only while
// it's the one chosen; it stays drawn while hidden, so what it reads keeps
// its summary current. Anywhere else (the shell's `#settings`, a page that
// borrows one) it's drawn in place, and its id is the section index's
// anchor; the index sits beside the column on a wide screen.
//
// A section can hold sections (Arugula #1105): where a scope merges a few
// into one row ("Runs and workspaces"), each it holds is a part of it,
// under a smaller heading, and tells it its summary instead of the list.
// The row says the parts' summaries in a line, where the section says
// none of its own. A scope's sections are listed flat, under no headings
// of their own (Arugula #1317).

import { createContext, type ComponentChildren, type RefObject } from "preact";
import { useContext, useEffect, useLayoutEffect, useMemo, useRef, useState } from "preact/hooks";

/** What a section tells the list it's in. */
export interface ListedSection {
  id: string;
  title: string;
  summary?: string;
  /** Its element, to put the list in the page's order. */
  el: HTMLElement;
}

/** The list a section is in: the one chosen, and where it says what it is. */
export interface SectionList {
  /** The section shown; the rest are drawn hidden. */
  shown: string | null;
  /** The section the address names, before it's drawn: a section that
   * opens with something started (a form) when it's asked for. */
  asked: string | null;
  tell(s: ListedSection): void;
  forget(id: string, el: HTMLElement): void;
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
    if (!el.current) return;
    if (whole) whole.tell(id, said, el.current);
    else list?.tell({ id, title, summary: said, el: el.current });
  }, [list, whole, id, title, said]);
  useLayoutEffect(() => {
    const at = el.current;
    return () => void (at && (whole ? whole.forget(id, at) : list?.forget(id, at)));
  }, [list, whole, id]);
  // A part shows whenever its section does, under a smaller heading.
  const hidden = list && !whole ? list.shown !== id : false;
  const Heading = whole ? "h3" : "h2";
  return (
    <section
      ref={el}
      class={whole ? "settings-part" : "settings-sec"}
      id={id}
      hidden={hidden}
      aria-labelledby={`${id}-h`}
      {...(whole ? { "data-settings-part": id } : { "data-settings-section": id, "data-settings-title": title })}
      data-settings-summary={said}
      {...hooks}
    >
      <Heading id={`${id}-h`}>
        {title}
        {count !== undefined ? <span class="dim">{count}</span> : null}
      </Heading>
      {lede ? <p class="settings-lede">{lede}</p> : null}
      {more ? <div class="settings-more">{more}</div> : null}
      <WholeContext.Provider value={holds}>{children}</WholeContext.Provider>
    </section>
  );
}

/** The section index, beside the column on a wide screen: the sections
 * the page shows, read off it as each one loads (some show only when
 * there's something in them), the one you're in marked (UI audit #166):
 * the topmost of those on the screen. */
export function SectionIndex({
  body,
  anchor = (id) => `#${id}`,
  opened: openedAt = () => decodeURIComponent(location.hash.slice(1)),
}: {
  body: RefObject<HTMLDivElement | null>;
  /** A section's address, where the hash isn't simply its id. */
  anchor?: (id: string) => string;
  /** The section the page was opened at. */
  opened?: () => string | undefined;
}) {
  const [secs, setSecs] = useState<{ id: string; title: string }[]>([]);
  const [here, setHere] = useState("");
  useEffect(() => {
    const el = body.current;
    if (!el) return;
    const read = () => {
      const next = [...el.querySelectorAll<HTMLElement>(".settings-sec")].map((s) => ({ id: s.id, title: s.dataset.settingsTitle ?? s.id }));
      setSecs((prev) => (prev.map((s) => s.id).join() === next.map((s) => s.id).join() ? prev : next));
    };
    read();
    const watch = new MutationObserver(read);
    watch.observe(el, { childList: true, subtree: true });
    return () => watch.disconnect();
  }, []);
  // A link to a section (`/settings#tokens`, from a project's settings)
  // goes to it once it has loaded, and stays on it while the sections
  // above it come in, for the page's first two seconds.
  const opened = useRef(Date.now());
  useEffect(() => {
    const id = openedAt();
    if (!id || Date.now() - opened.current > 2000 || !secs.some((s) => s.id === id)) return;
    document.getElementById(`${id}-h`)?.scrollIntoView({ block: "start" });
  }, [secs]);
  useEffect(() => {
    const el = body.current;
    if (!el || typeof IntersectionObserver === "undefined") return;
    const seen = new Set<Element>();
    const look = new IntersectionObserver((entries) => {
      for (const e of entries) e.isIntersecting ? seen.add(e.target) : seen.delete(e.target);
      const top = secs.find((s) => [...seen].some((x) => x.id === s.id));
      if (top) setHere(top.id);
    });
    for (const s of secs) {
      const sec = document.getElementById(s.id);
      if (sec) look.observe(sec);
    }
    return () => look.disconnect();
  }, [secs]);
  // To the section's heading, which takes the focus for a keyboard.
  const go = (e: MouseEvent, id: string) => {
    const h = document.getElementById(`${id}-h`);
    if (!h) return;
    e.preventDefault();
    h.scrollIntoView({ block: "start" });
    h.tabIndex = -1;
    h.focus({ preventScroll: true });
  };
  return (
    <nav class="settings-index" aria-label="Settings sections">
      <ul>
        {secs.map((s) => (
          <li key={s.id}>
            <a href={anchor(s.id)} aria-current={here === s.id ? "location" : undefined} onClick={(e) => go(e, s.id)}>
              {s.title}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}
