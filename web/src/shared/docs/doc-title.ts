// What a Doc's title may be (#979), as the platform and the daemon's `title_ok`
// check it: trimmed, not empty, one line, at most 200 characters. The web
// checks first so a bad one never leaves the page.

export const TITLE_MAX = 200;

/** Why `title` can't be a Doc's, or null. */
export function titleRefusal(title: string): string | null {
  const t = title.trim();
  if (!t) return "A Doc needs a title.";
  if (/[\r\n]/.test(t)) return "A title is one line.";
  if ([...t].length > TITLE_MAX) return `The title is over ${TITLE_MAX} characters: shorten it.`;
  return null;
}
