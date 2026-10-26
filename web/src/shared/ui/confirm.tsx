// Removals that can't be undone (a machine, a device, a team member, a
// passkey) ask in their own dialog. They used to ask in place, on the
// button itself, so a double-click removed a machine for good (#328).

import { useEffect, useRef, useState } from "preact/hooks";
import { useModal } from "./modal";

/** How long the dialog ignores clicks after it opens: the second click of
 * a double-click lands here. */
const SETTLE_MS = 500;

/** Over whatever panel is open. Cancel, or a click outside, keeps it. */
export function ConfirmRemove({ title, children, label = "Remove", go, cancel }: { title: string; children: preact.ComponentChildren; label?: string; go: () => void; cancel: () => void }) {
  const [ready, setReady] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  // Over the dialog it came from: Escape cancels this one only (UI audit #150).
  useModal(ref, cancel);
  useEffect(() => {
    const t = setTimeout(() => setReady(true), SETTLE_MS);
    return () => clearTimeout(t);
  }, []);
  return (
    <div class="prompt-backdrop" onClick={(e) => ready && e.target === e.currentTarget && cancel()}>
      <div ref={ref} class="prompt control-prompt" role="alertdialog" aria-modal="true" aria-label={title} data-confirm-dialog>
        <h2>{title}</h2>
        {children}
        <div class="prompt-buttons">
          <button data-cancel-remove onClick={cancel}>
            Cancel
          </button>
          <button class="danger" data-confirm-remove disabled={!ready} onClick={go}>
            {label}
          </button>
        </div>
      </div>
    </div>
  );
}
