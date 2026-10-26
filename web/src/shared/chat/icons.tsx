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
