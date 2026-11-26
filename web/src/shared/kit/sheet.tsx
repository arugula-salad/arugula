// Sheet (kit §4.1, 12): the one overlay, over the backdrop: Go to…, the
// keys, Look, a menu, a confirm. Focus goes in and stays in; Esc or a click
// on the backdrop closes it, and focus goes back where it was.

import type { ComponentChildren } from "preact";
import { useEffect, useRef } from "preact/hooks";
import { Surface } from "./surface";
import "./kit.css";

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])';

export function Sheet({ title, onClose, children }: { title: string; onClose: () => void; children: ComponentChildren }) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const was = document.activeElement as HTMLElement | null;
    box.current?.querySelector<HTMLElement>(FOCUSABLE)?.focus();
    return () => was?.focus();
  }, []);
  const key = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      onClose();
    } else if (e.key === "Tab") {
      const all = [...(box.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])];
      const [first, last] = [all[0], all.at(-1)];
      if (e.shiftKey && document.activeElement === first) (e.preventDefault(), last?.focus());
      else if (!e.shiftKey && document.activeElement === last) (e.preventDefault(), first?.focus());
    }
  };
  return (
    <div class="k-sheet-back" onClick={(e) => !(e.target as HTMLElement).closest(".k-sheet") && onClose()} onKeyDown={key}>
      <div ref={box} role="dialog" aria-modal="true" aria-label={title}>
        <Surface kind="overlay" class="k-sheet" data-kit="sheet">
          <h2 class="k-sheet-title">{title}</h2>
          {children}
        </Surface>
      </div>
    </div>
  );
}
