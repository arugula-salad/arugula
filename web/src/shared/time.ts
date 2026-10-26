// Dates and times, one way on both surfaces (UI audit #182): how long ago
// for the last week ("3m ago", "2d ago"), then the day ("Oct 23", with the
// year when it isn't this one), and the full time ("Oct 23, 2026, 1:24 PM")
// in the title. Times to come read "in 7h". `<Time>` (./ui/time.tsx) draws
// one; these are its words, for a sentence or a test.

const MIN = 60;
const HOUR = 3600;
const DAY = 86_400;
const WEEK = 7 * DAY;

/** "Oct 23", or "Oct 23, 2025" when it isn't this year. */
export function day(at: number, now = Date.now()): string {
  const d = new Date(at);
  return d.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    ...(d.getFullYear() === new Date(now).getFullYear() ? {} : { year: "numeric" }),
  });
}

/** The full time, for a title: "Oct 23, 2026, 1:24 PM"; with `seconds`,
 * "Oct 23, 2026, 1:24:05 PM". */
export function when(at: number, seconds = false): string {
  return new Date(at).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
    ...(seconds ? { second: "2-digit" } : {}),
  });
}

/** How a time reads: `short` ("5m ago", "Oct 23"); `on`, the same in a
 * sentence ("on Oct 23"); `long`, a sentence with its units spelled out
 * ("5 minutes ago", "on Oct 23"), as an issue's "opened 3 days ago by kim". */
export type Style = "short" | "on" | "long";

/** "just now", "5m ago", "3h ago", "2d ago", then the day; a time to come
 * is "in 2m", "in 7h", "in 2d", then the day. With `seconds` the last
 * minute counts them ("12s ago"), for events that land seconds apart. */
export function ago(at: number, now = Date.now(), style: Style = "short", seconds = false): string {
  const s = Math.floor((now - at) / 1000);
  const future = s < 0;
  const a = Math.abs(s);
  if (a >= WEEK) return style === "short" ? day(at, now) : `on ${day(at, now)}`;
  // A minute ahead is another clock's skew, not a time to come.
  if (s > -60 && s < (seconds ? 1 : 45)) return "just now";
  const [v, unit] = seconds && a < MIN ? [a, "s"] : a < HOUR ? [Math.max(1, Math.floor(a / MIN)), "m"] : a < DAY ? [Math.floor(a / HOUR), "h"] : [Math.floor(a / DAY), "d"];
  const words = style === "long" ? ` ${{ s: "second", m: "minute", h: "hour", d: "day" }[unit]}${v === 1 ? "" : "s"}` : unit;
  return future ? `in ${v}${words}` : `${v}${words} ago`;
}

/** An ISO time for `<time dateTime>`. */
export const iso = (at: number): string => new Date(at).toISOString();
