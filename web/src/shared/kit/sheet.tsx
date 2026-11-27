// Sheet (kit §4.1, 12): the one overlay, over the backdrop: Go to…, the
// keys, Look, a menu, a confirm; with `side`, a drawer at the right
// (Running, Lift 6a). Focus goes in and stays in; Esc or a click on the
// backdrop closes it, and focus goes back where it was.

import type { ComponentChildren } from "preact";
import { useLayoutEffect, useRef } from "preact/hooks";
import { useModal } from "../ui/modal";
import { Surface } from "./surface";
import "./kit.css";

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])';

/** `titled`: its title shows (Go to…'s field is its own head: the title
 * names the dialog only). `returnFocus`: whether focus goes back when it
 * shuts (not after Go to… ran something that put it elsewhere). It's a
 * dialog as every other is (useModal): one at a time for what opens by
 * itself (Arugula #280), focus kept in, Escape before the terminal. */
export function Sheet({ title, titled = true, side = false, returnFocus, onClose, children }: { title: string; titled?: boolean; /** At the right, the window's height (the Running drawer, Lift 6a), not centred near the top. */ side?: boolean; returnFocus?: () => boolean; onClose: () => void; children: ComponentChildren }) {
  const box = useRef<HTMLDivElement>(null);
  useModal(box, onClose, true, undefined, returnFocus);
  // Into its first control, unless one asks for the focus.
  useLayoutEffect(() => {
    const el = box.current;
    if (el && document.activeElement === el) el.querySelector<HTMLElement>(FOCUSABLE)?.focus();
  }, []);
  return (
    <div class={side ? "k-sheet-back is-side" : "k-sheet-back"} onClick={(e) => !(e.target as HTMLElement).closest(".k-sheet") && onClose()}>
      <div ref={box} role="dialog" aria-modal="true" aria-label={title}>
        <Surface kind="overlay" class={side ? "k-sheet is-side" : "k-sheet"} data-kit="sheet" data-sheet-side={side ? "" : undefined}>
          {titled && <h2 class="k-sheet-title">{title}</h2>}
          {children}
        </Surface>
      </div>
    </div>
  );
}
