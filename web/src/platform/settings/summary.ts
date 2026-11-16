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

/** Which nights, in words: `Weeknights`, `Every night`, `Mon, Wed`. */
function nights(days: number[]): string {
  const set = [...new Set(days)].sort((a, b) => a - b).join();
  if (set === "1,2,3,4,5,6,7") return "Every night";
  if (set === "1,2,3,4,5") return "Weeknights";
  if (set === "6,7") return "Weekends";
  return [...new Set(days)]
    .sort((a, b) => a - b)
    .map((d) => DAY[d - 1])
    .join(", ");
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
