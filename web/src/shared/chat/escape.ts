/** Chat's Escape closers call this once they've taken the key. An Escape the
 * page leaves unhandled goes on to the desktop app's window, which leaves full
 * screen on it (#380). */
export function handled(e: Event) {
  e.preventDefault();
  e.stopPropagation();
}

/** A keydown handler that closes on Escape, and marks the key handled. */
export const closeOnEscape = (close: () => void) => (e: KeyboardEvent) => {
  if (e.key !== "Escape") return;
  handled(e);
  close();
};
