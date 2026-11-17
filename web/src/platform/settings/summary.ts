// What a section of Settings says in its list (Arugula #1104): a few words
// on what's set, read from what the section already loads. The words that
// more than one section uses are here, and quiet hours' (the only one
// with a rule to it), tested in scripts/settings-summary.test.mjs.

import type { QuietHours } from "../../shared/proto.gen";

/** `3 repos`, `1 repo`. */
export const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`;

/** A few names, then how many more: `deploy, outage +2`. */
export function some(names: string[], shown = 2): string {
  const more = names.length - shown;
  return names.slice(0, shown).join(", ") + (more > 0 ? ` +${more}` : "");
}

/** Parts that say something, joined: `Team plan · 6 seats`. */
export const joined = (...parts: (string | false | null | undefined)[]) => parts.filter(Boolean).join(" · ");

/** A span of minutes in words: `45 min`, `1h 30m`, `2 hours`, `2 days`. */
export function span(minutes: number): string {
  if (minutes < 60 || minutes % 60 !== 0) return minutes < 60 ? `${minutes} min` : `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
  const hours = minutes / 60;
  return hours % 24 === 0 ? plural(hours / 24, "day") : plural(hours, "hour");
}

const DAY = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/** Minutes past midnight as people say them: `10 PM`, `7:30 AM`. */
export function spoken(m: number): string {
  const h = Math.floor(m / 60) % 24;
  const min = m % 60;
  return `${h % 12 || 12}${min ? `:${String(min).padStart(2, "0")}` : ""} ${h < 12 ? "AM" : "PM"}`;
}

/** Which nights, in words: `Weeknights`, `Weekends`, `Every night`; three
 * or more in a row as a range (`Mon–Sat`, and `Sun–Thu` round the week's
 * end when that's all of them), the rest one by one (`Mon, Wed, Fri`). */
function nights(days: number[]): string {
  const on = [...new Set(days)].filter((d) => d >= 1 && d <= 7).sort((a, b) => a - b);
  const set = on.join();
  if (set === "1,2,3,4,5,6,7") return "Every night";
  if (set === "1,2,3,4,5") return "Weeknights";
  if (set === "6,7") return "Weekends";
  // Runs of days in a row, Monday first.
  const runs: number[][] = [];
  for (const d of on) {
    const last = runs[runs.length - 1];
    if (last && d === last[last.length - 1] + 1) last.push(d);
    else runs.push([d]);
  }
  // One run that goes past Sunday into Monday: Sunday's end is its start.
  if (runs.length === 2 && runs[0][0] === 1 && runs[1][runs[1].length - 1] === 7) runs.splice(0, 2, [...runs[1], ...runs[0]]);
  return runs.map((r) => (r.length >= 3 ? `${DAY[r[0] - 1]}–${DAY[r[r.length - 1] - 1]}` : r.map((d) => DAY[d - 1]).join(", "))).join(", ");
}

/** Quiet hours in a line: `Weeknights 10 PM – 8 AM · New York`, or `Off`.
 * Windows with times of their own say the nights and that the times vary. */
export function quietSummary(quiet: QuietHours[], tz: string): string {
  const on = quiet.filter((q) => q.days.length > 0);
  if (on.length === 0) return "Off";
  const zone = tz.split("/").pop()!.replace(/_/g, " ");
  const days = nights(on.flatMap((q) => q.days));
  if (on.length > 1) return joined(`${days}, times vary`, zone);
  return joined(`${days} ${spoken(on[0].from)} – ${spoken(on[0].to)}`, zone);
}
