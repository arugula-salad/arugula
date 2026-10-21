// The picker's, the palette's and chat's switcher's fuzzy match.

/**
 * How well `query` matches `text` as a subsequence (case-insensitive):
 * higher is better, null if it doesn't. Runs of letters and matches at the
 * start of a word count more, and a shorter text wins a tie.
 */
export function fuzzy(query: string, text: string): number | null {
  const q = query.toLowerCase();
  const t = text.toLowerCase();
  if (!q) return 0;
  let score = 0;
  let at = 0;
  let run = 0;
  for (const ch of q) {
    const i = t.indexOf(ch, at);
    if (i < 0) return null;
    run = i === at ? run + 1 : 1;
    const wordStart = i === 0 || "/-_. ".includes(t[i - 1]);
    score += 1 + run * 2 + (wordStart ? 6 : 0) - Math.min(i - at, 5);
    at = i + 1;
  }
  if (t.startsWith(q)) score += 10;
  return score - t.length / 100;
}
