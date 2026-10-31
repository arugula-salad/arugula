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
