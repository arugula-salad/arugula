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

/** A stretch of one day, in minutes past its midnight: from `from` up to
 * `to` (1440 is the next midnight). */
export type Span = { from: number; to: number };

/** The week windows of days and times cover, Monday to Sunday: each day's
 * stretches, in order, joined where they touch. A window that runs
 * overnight starts on its day and ends on the next morning (Sunday's runs
 * into Monday), as QuietHours' do; one that ends where it starts is the
 * whole day round. A time that isn't one (a field mid-edit) draws nothing. */
export function weekSpans(windows: readonly { days: readonly number[]; from: string; to: string }[]): Span[][] {
  const week: Span[][] = Array.from({ length: 7 }, () => []);
  for (const w of windows) {
    const a = clockMinutes(w.from);
    const b = clockMinutes(w.to);
    if (Number.isNaN(a) || Number.isNaN(b)) continue;
    for (const d of w.days) {
      if (d < 1 || d > 7) continue;
      if (b > a) week[d - 1].push({ from: a, to: b });
      else {
        week[d - 1].push({ from: a, to: 1440 });
        if (b > 0) week[d % 7].push({ from: 0, to: b });
      }
    }
  }
  return week.map((spans) =>
    spans
      .sort((x, y) => x.from - y.from)
      .reduce<Span[]>((out, s) => {
        const last = out[out.length - 1];
        if (last && s.from <= last.to) last.to = Math.max(last.to, s.to);
        else out.push({ ...s });
        return out;
      }, []),
  );
}

/** Minutes past midnight as a clock reads them: "08:00", "24:00". */
export function clockOf(m: number): string {
  return `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
}

/** Windows of days back to one: every day they cover, at the first one's
 * times. */
export function sameTimes<W extends { days: number[] }>(windows: readonly W[]): W[] {
  if (windows.length === 0) return [];
  const days = [...new Set(windows.flatMap((w) => w.days))].sort((a, b) => a - b);
  return [{ ...windows[0], days }];
}
