// The values Weekdays and TimeRange edit (./schedule.tsx, Arugula #1101),
// kept apart from the components so node can test them. A day is 1 (Monday)
// to 7 (Sunday), as QuietHours' are; a time is "HH:MM", as an
// `<input type="time">`'s is.

/** The seven days, Monday first: the chip's letter and the day's name. */
export const WEEKDAYS: { day: number; letter: string; name: string }[] = [
  { day: 1, letter: "M", name: "Monday" },
  { day: 2, letter: "T", name: "Tuesday" },
  { day: 3, letter: "W", name: "Wednesday" },
  { day: 4, letter: "T", name: "Thursday" },
  { day: 5, letter: "F", name: "Friday" },
  { day: 6, letter: "S", name: "Saturday" },
  { day: 7, letter: "S", name: "Sunday" },
];

/** `days` with `day` turned on or off: each day once, in week order. */
export function toggleDay(days: readonly number[], day: number): number[] {
  const set = new Set(days);
  if (set.has(day)) set.delete(day);
  else set.add(day);
  return [...set].filter((d) => d >= 1 && d <= 7).sort((a, b) => a - b);
}

/** Minutes past midnight of "HH:MM" (NaN when it isn't a time). */
export function clockMinutes(t: string): number {
  const m = /^(\d{1,2}):(\d{2})$/.exec(t);
  if (!m) return NaN;
  const h = Number(m[1]);
  const min = Number(m[2]);
  return h < 24 && min < 60 ? h * 60 + min : NaN;
}

/** Whether a range ends at or before it starts: it runs into the next
 * morning (22:00 to 08:00). */
export function overnight(from: string, to: string): boolean {
  const a = clockMinutes(from);
  const b = clockMinutes(to);
  return !Number.isNaN(a) && !Number.isNaN(b) && b <= a;
}

/** Windows of days, with `days` given to window `i`: a day is in one window
 * only, so the others give it up. A later window that's left with no days
 * goes; the first always stays, so there's one to pick days in. */
export function claimDays<W extends { days: number[] }>(windows: readonly W[], i: number, days: readonly number[]): W[] {
  const mine = new Set(days);
  return windows
    .map((w, j) => (j === i ? { ...w, days: [...days] } : { ...w, days: w.days.filter((d) => !mine.has(d)) }))
    .filter((w, j) => j === 0 || j === i || w.days.length > 0);
}
