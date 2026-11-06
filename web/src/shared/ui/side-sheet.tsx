// The side sheet (#690): a panel at the right edge that shows one thing
// beside the page, and a bottom sheet at 600px and under. Boards' row drawer
// is the first; the rest of the page sits under `--backdrop` while it is up.
//
// Like a dialog, it takes the focus and keeps it (Tab wraps), Escape closes
// it before a terminal under it can hear the key, and the focus goes back to
// what opened it (`useModal`). A click on the backdrop closes it too, at
// every width: on a desktop a click on another card closes the sheet first.
// It has a head (a title, an optional subtitle, the ×), a body that scrolls
// and an optional foot of buttons, laid out like a dialog's.

import { useLayoutEffect, useRef } from "preact/hooks";
import { useModal } from "./modal";
import "./side-sheet.css";

type Attrs = Record<string, string | boolean>;

let sheets = 0;

export function SideSheet({
  title,
  subtitle,
  close,
  children,
  foot,
  class: cls,
  root,
  titleAttrs,
  closeAttrs,
}: {
  title: string;
  subtitle?: string;
  close: () => void;
  children: preact.ComponentChildren;
  /** Buttons for the foot, Cancel first and the action last. */
  foot?: preact.ComponentChildren;
  /** Added to the sheet's own class, for what a page styles inside it. */
  class?: string;
  /** Attributes for the sheet itself, such as a `data-` hook for the specs. */
  root?: Attrs;
  titleAttrs?: Attrs;
  closeAttrs?: Attrs;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // A press that starts in the sheet and ends on the backdrop (a drag that selects text) isn't a click on it.
  const down = useRef(false);
  const id = useRef(`side-sheet-${++sheets}`).current;
  useModal(ref, close);
  // The body scrolls from the top for each thing the sheet shows.
  useLayoutEffect(() => void ref.current?.querySelector(".side-sheet-body")?.scrollTo?.(0, 0), [title]);
  return (
    <div class="side-sheet-backdrop" data-side-sheet-backdrop onPointerDown={(e) => (down.current = e.target === e.currentTarget)} onClick={(e) => down.current && e.target === e.currentTarget && close()}>
      <div ref={ref} class={cls ? `side-sheet ${cls}` : "side-sheet"} role="dialog" aria-modal="true" aria-labelledby={id} {...root}>
        <header class="side-sheet-head">
          <div class="side-sheet-titles">
            <h2 id={id} {...titleAttrs}>
              {title}
            </h2>
            {subtitle && <p class="side-sheet-sub">{subtitle}</p>}
          </div>
          <button type="button" class="bar-btn sq" aria-label="Close" title="Close" onClick={close} {...closeAttrs}>
            ×
          </button>
        </header>
        <div class="side-sheet-body">{children}</div>
        {foot && <footer class="side-sheet-foot">{foot}</footer>}
      </div>
    </div>
  );
}
