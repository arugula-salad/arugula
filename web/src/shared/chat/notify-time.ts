// The clock side of notification settings (#115), with no page or network
// in it so a test can run it: snooze presets and what a snooze box accepts.

const MIN = 60_000;

/** Snooze presets, by their label, as the time they end (ms). "Until
 * tomorrow" is 8 in the morning, on this device's clock. */
export function snoozePresets(now = Date.now()): { label: string; until: number }[] {
  const tomorrow = new Date(now);
  tomorrow.setDate(tomorrow.getDate() + 1);
  tomorrow.setHours(8, 0, 0, 0);
  return [
    { label: "30 minutes", until: now + 30 * MIN },
    { label: "1 hour", until: now + 60 * MIN },
    { label: "2 hours", until: now + 120 * MIN },
    { label: "Until tomorrow", until: tomorrow.getTime() },
  ];
}

/** "until 3:40 PM", or with the day when it isn't today. */
export function untilText(until: number, now = Date.now()): string {
  const d = new Date(until);
  const time = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  return new Date(now).toDateString() === d.toDateString() ? `until ${time}` : `until ${d.toLocaleDateString([], { weekday: "short" })} ${time}`;
}

/** "45m", "2h" or "17:30" (today, or tomorrow if that's past) as the time
 * it ends (ms); null if it isn't one. */
export function parseSnooze(text: string, now = Date.now()): number | null {
  const t = text.trim().toLowerCase();
  const span = /^(\d+(?:\.\d+)?)\s*(m|min|h|hr|hours?|minutes?)$/.exec(t);
  if (span) return now + Number(span[1]) * (span[2].startsWith("h") ? 3_600_000 : 60_000);
  const at = /^(\d{1,2}):(\d{2})\s*(am|pm)?$/.exec(t) ?? /^(\d{1,2})()\s*(am|pm)$/.exec(t);
  if (!at) return null;
  let h = Number(at[1]);
  const m = Number(at[2] || 0);
  if (at[3] === "pm" && h < 12) h += 12;
  if (at[3] === "am" && h === 12) h = 0;
  if (h > 23 || m > 59) return null;
  const d = new Date(now);
  d.setHours(h, m, 0, 0);
  if (d.getTime() <= now) d.setDate(d.getDate() + 1);
  return d.getTime();
}

export const deviceZone = () => Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";

/** Minutes after midnight as "HH:MM" for a time input, and back. */
export const clock = (m: number) => `${String(Math.floor(m / 60)).padStart(2, "0")}:${String(m % 60).padStart(2, "0")}`;
export const minutes = (hhmm: string) => {
  const [h, m] = hhmm.split(":").map(Number);
  return (h || 0) * 60 + (m || 0);
};
