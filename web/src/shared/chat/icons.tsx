// Chat's drawn icons (UI audit #169), where a text glyph reads as
// something else or as nothing at 13px: a bookmark for Saved (☆ is a
// room's star) and a magnifier for the channel browser (⌕ was a smudge).
// Each follows the text's colour.

export function BookmarkIcon({ on, size = 14 }: { on?: boolean; size?: number }) {
  return (
    <svg viewBox="0 0 16 16" width={size} height={size} aria-hidden="true" fill={on ? "currentColor" : "none"} stroke="currentColor" stroke-width="1.4" stroke-linejoin="round">
      <path d="M4 2.5h8v11l-4-3-4 3z" />
    </svg>
  );
}

/** Add a reaction: a smiley, on the hover bar and the reaction row (#501; ☺
 * read as `@` at text size). */
export function ReactIcon({ size = 14 }: { size?: number }) {
  return (
    <svg viewBox="0 0 16 16" width={size} height={size} aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4">
      <circle cx="8" cy="8" r="6.2" />
      <path d="M5.4 9.6c.6 1.1 1.5 1.6 2.6 1.6s2-.5 2.6-1.6" stroke-linecap="round" />
      <path d="M5.9 6.2v.6M10.1 6.2v.6" stroke-linecap="round" stroke-width="1.8" />
    </svg>
  );
}

export function SearchIcon({ size = 14 }: { size?: number }) {
  return (
    <svg viewBox="0 0 16 16" width={size} height={size} aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round">
      <circle cx="7" cy="7" r="4.5" />
      <path d="M10.5 10.5 14 14" />
    </svg>
  );
}

/** A machine's agent (#428): beside the machine in the `@` picker and in a
 * message that mentions it, and on the room's agents chip. */
export function MachineIcon({ size = 15 }: { size?: number }) {
  return (
    <svg class="machine-icon" viewBox="0 0 16 16" width={size} height={size} aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
      <rect x="3" y="5.5" width="10" height="7" rx="2" />
      <path d="M8 5.5V3M6.5 3h3" />
      <circle cx="6" cy="9" r=".6" fill="currentColor" />
      <circle cx="10" cy="9" r=".6" fill="currentColor" />
    </svg>
  );
}

/** What the message menu's rows carry (#757): strokes in the text's colour,
 * sized by the menu's lead column, in place of the emoji they were. */
const Stroke = ({ children }: { children: preact.ComponentChildren }) => (
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round">
    {children}
  </svg>
);

export const QuoteIcon = () => (
  <Stroke>
    <path d="M3 4h5v4H5.5C5.5 10 4.5 11 3 11.5M9 4h5v4h-2.5c0 2-1 3-2.5 3.5" />
  </Stroke>
);

export const LinkIcon = () => (
  <Stroke>
    <path d="M6.5 9.5a2.5 2.5 0 0 0 3.5 0l2-2a2.5 2.5 0 0 0-3.5-3.5l-.7.7M9.5 6.5a2.5 2.5 0 0 0-3.5 0l-2 2a2.5 2.5 0 0 0 3.5 3.5l.7-.7" />
  </Stroke>
);

export const UnreadIcon = () => (
  <Stroke>
    <circle cx="8" cy="8" r="5.5" />
    <path d="M8 2.5v11" />
  </Stroke>
);

export const ClockIcon = () => (
  <Stroke>
    <circle cx="8" cy="8" r="5.5" />
    <path d="M8 5v3.2l2 1.3" />
  </Stroke>
);

export const PinIcon = () => (
  <Stroke>
    <path d="M6 2.5h4l-.5 4 2 2.5H4.5l2-2.5zM8 9v4.5" />
  </Stroke>
);

export const EditIcon = () => (
  <Stroke>
    <path d="M10.5 3l2.5 2.5L6 12.5l-3 .5.5-3z" />
  </Stroke>
);

export const TrashIcon = () => (
  <Stroke>
    <path d="M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.5 8.5h6l.5-8.5" />
  </Stroke>
);

export const ThreadIcon = () => (
  <Stroke>
    <path d="M6.5 3.5 3 7l3.5 3.5M3 7h6a4 4 0 0 1 4 4v1.5" />
  </Stroke>
);
