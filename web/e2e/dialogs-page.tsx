// The fixture page for the dialog and the side sheet (#690): a Modal, a
// ConfirmRemove and a SideSheet over a page with a field that listens for
// Escape the way a terminal pane does, so the spec can tell that Escape
// closed the dialog and went no further. dialogs.spec.ts builds this with
// Vite and serves it, so no daemon is involved.

import { render } from "preact";
import { useState } from "preact/hooks";
import "../src/shared/style.css";
import "../src/shared/buttons.css";
import { Modal } from "../src/shared/ui/modal";
import { ConfirmRemove } from "../src/shared/ui/confirm";
import { SideSheet } from "../src/shared/ui/side-sheet";

function Page() {
  const [open, setOpen] = useState<"modal" | "confirm" | "sheet" | "attention" | null>(null);
  const close = () => setOpen(null);
  return (
    <main style="padding:24px">
      <button type="button" class="secondary" data-open="modal" onClick={() => setOpen("modal")}>
        Open dialog
      </button>
      <button type="button" class="secondary" data-open="confirm" onClick={() => setOpen("confirm")}>
        Open confirm
      </button>
      <button type="button" class="secondary" data-open="sheet" onClick={() => setOpen("sheet")}>
        Open sheet
      </button>
      <button type="button" class="secondary" data-open="attention" onClick={() => setOpen("attention")}>
        Open attention
      </button>
      <p>
        <input data-pane-field aria-label="A pane's field" />
      </p>
      {open === "modal" && (
        <Modal
          title="Rename the pane"
          close={close}
          buttons={
            <>
              <button type="button" class="secondary" onClick={close}>
                Cancel
              </button>
              <button type="button" class="primary" data-ok onClick={close}>
                Rename
              </button>
            </>
          }
        >
          <input aria-label="Name" data-autofocus />
        </Modal>
      )}
      {open === "confirm" && (
        <ConfirmRemove title="Close this pane?" go={close} cancel={close}>
          <p>What runs in it stops.</p>
        </ConfirmRemove>
      )}
      {open === "sheet" && (
        <SideSheet title="A row with a long title that wraps over more than one line" close={close} root={{ "data-sheet": true }}>
          <a href="#one">One</a> <button type="button">Two</button> <input aria-label="Three" />
        </SideSheet>
      )}
      {open === "attention" && (
        <SideSheet
          class="ws-att"
          title="Needs you"
          subtitle="nothing waiting"
          close={close}
          root={{ "data-attention-drawer": true }}
          foot={
            <>
              <span>Looking here answers nothing.</span>
              <button type="button" class="link">
                Notifications…
              </button>
            </>
          }
        >
          <div class="ws-att-list" role="listbox" aria-label="What needs you" tabIndex={-1} />
        </SideSheet>
      )}
    </main>
  );
}

// What reaches the page after the dialog's own handlers: Escape, as a pane's terminal would hear it.
(window as unknown as { escapes: number }).escapes = 0;
window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") (window as unknown as { escapes: number }).escapes++;
});

render(<Page />, document.getElementById("root")!);
