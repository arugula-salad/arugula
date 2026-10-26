// A section of /settings and of a project's settings: a display heading
// (with a count, where it lists things), one plain line saying what it is,
// and the rest of what there is to know behind More (a <details>), then
// what it holds. Its id is the section index's anchor; the index sits
// beside the column on a wide screen (../settings.css).

import type { ComponentChildren, RefObject } from "preact";
import { useEffect, useRef, useState } from "preact/hooks";

export function Section({
  id,
  title,
  count,
  lede,
  more,
  hooks,
  children,
}: {
  id: string;
  title: string;
  /** How many it lists, beside the heading. */
  count?: number;
  lede: ComponentChildren;
  more?: ComponentChildren;
  /** The section's own `data-*` hooks. */
  hooks?: Record<`data-${string}`, string>;
  children?: ComponentChildren;
}) {
  return (
    <section class="settings-sec" id={id} aria-labelledby={`${id}-h`} data-settings-section={id} data-settings-title={title} {...hooks}>
      <h2 id={`${id}-h`}>
        {title}
        {count !== undefined ? <span class="dim">{count}</span> : null}
      </h2>
      <p class="settings-lede">{lede}</p>
      {more ? (
        <details class="settings-more">
          <summary>More</summary>
          {more}
        </details>
      ) : null}
      {children}
    </section>
  );
}

/** The section index, beside the column on a wide screen: the sections
 * the page shows, read off it as each one loads (some show only when
 * there's something in them), the one you're in marked (UI audit #166):
 * the topmost of those on the screen. */
export function SectionIndex({ body }: { body: RefObject<HTMLDivElement | null> }) {
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
    const id = decodeURIComponent(location.hash.slice(1));
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
            <a href={`#${s.id}`} aria-current={here === s.id ? "location" : undefined} onClick={(e) => go(e, s.id)}>
              {s.title}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}
