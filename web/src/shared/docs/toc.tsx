// A Doc's contents (Lift 6b, Arugula #1484), beside its paper on its page:
// On this page, its written sections' headings, each scrolling the Doc to
// it, the one being read marked as it scrolls (#1135) and ◧ on one that
// holds a figure; then Linked, what the Doc is on (its boards).

import type { ComponentChildren, RefObject } from "preact";
import { useEffect, useState } from "preact/hooks";
import type { ProjectDocSection } from "../proto";

/** A written section, as the contents list it. */
export interface TocSection {
  heading: string;
  /** It holds a chart, a drawing, a prototype or a picture. */
  figure: boolean;
}

/** A project Doc's written sections, for its contents. */
export const tocOf = (sections: ProjectDocSection[]): TocSection[] =>
  sections.filter((s) => s.body !== null).map((s) => ({ heading: s.heading, figure: /^(```(chart|svg|prototype)|!\[)/m.test(s.body ?? "") }));

/** How far under the top of the scroll a section's top may be and still be
 * the one being read. */
const READING_EDGE = 48;

/** The nearest box around `el` that scrolls. */
function scrollerOf(el: HTMLElement): HTMLElement | null {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const y = getComputedStyle(p).overflowY;
    if (y === "auto" || y === "scroll") return p;
  }
  return null;
}

/** Which section of the Doc in `reader` is being read: the last whose top
 * has reached the top of the scroll, or the last once it's at the end. */
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

export function DocToc({ sections, reader, linked }: { sections: TocSection[]; reader: RefObject<HTMLElement | null>; linked: ComponentChildren[] }) {
  const at = useReading(reader, sections.length);
  if (!sections.length && !linked.length) return null;
  const go = (i: number) => reader.current?.querySelectorAll("[data-section]")[i]?.scrollIntoView({ block: "start" });
  return (
    <aside class="doc-toc" aria-label="Contents" data-doc-toc>
      {sections.length > 0 && (
        <>
          <h2 class="doc-toc-head">
            On this page <span class="m-meta">{sections.length}</span>
          </h2>
          <ol class="doc-toc-list">
            {sections.map((s, i) => (
              <li key={i}>
                <button type="button" class="doc-toc-go" title={s.heading} aria-current={i === at ? "location" : undefined} data-doc-toc-go onClick={() => go(i)}>
                  <span>{s.heading}</span>
                  {s.figure && (
                    <span class="m-meta" title="Holds a figure" data-doc-toc-fig>
                      ◧
                    </span>
                  )}
                </button>
              </li>
            ))}
          </ol>
        </>
      )}
      {linked.length > 0 && (
        <>
          <h2 class="doc-toc-head">
            Linked <span class="m-meta">{linked.length}</span>
          </h2>
          <ul class="doc-toc-list">{linked}</ul>
        </>
      )}
    </aside>
  );
}
