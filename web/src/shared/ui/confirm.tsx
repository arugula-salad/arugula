// Removals that can't be undone (a machine, a device, a team member, a
// passkey) ask in their own dialog. They used to ask in place, on the
// button itself, so a double-click removed a machine for good (#328).

import { useEffect, useState } from "preact/hooks";
import { Modal } from "./modal";

/** How long the dialog ignores clicks after it opens: the second click of
 * a double-click lands here. */
const SETTLE_MS = 500;

/** Over whatever panel is open. Cancel, or a click outside, keeps it. */
export function ConfirmRemove({
  title,
  children,
  label = "Remove",
  go,
  cancel,
  busy = false,
}: {
  title: string;
  children: preact.ComponentChildren;
  label?: string;
  go: () => void;
  cancel: () => void;
  /** Not yet: what it needs is still coming (the button waits). */
  busy?: boolean;
}) {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const t = setTimeout(() => setReady(true), SETTLE_MS);
    return () => clearTimeout(t);
  }, []);
  // Over the dialog it came from: Escape cancels this one only (UI audit #150).
  return (
    <Modal
      alert
      x={false}
      title={title}
      label={title}
      close={cancel}
      backdropClose={ready}
      root={{ "data-confirm-dialog": true }}
      buttons={
        <>
          <button class="secondary" data-cancel-remove onClick={cancel}>
            Cancel
          </button>
          <button class="primary danger" data-confirm-remove disabled={!ready || busy} onClick={go}>
            {label}
          </button>
        </>
      }
    >
      {children}
    </Modal>
  );
}
