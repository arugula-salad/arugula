// A dialog on control's page: the trust screens' (web/src/shell/trust/)
// and the platform's settings (web/src/platform/settings/) alike, so both
// sides draw it the same (M149).

/** A control dialog: as tall as the window allows, scrolling inside, with
 * its close button pinned where it can always be seen. */
export function Modal({ children, close }: { children: preact.ComponentChildren; close?: () => void }) {
  return (
    <div class="prompt-backdrop" onClick={(e) => e.target === e.currentTarget && close?.()}>
      <div class="prompt control-prompt">
        {close ? (
          <button class="modal-close" aria-label="Close dialog" title="Close" onClick={close}>
            ×
          </button>
        ) : null}
        {children}
      </div>
    </div>
  );
}
