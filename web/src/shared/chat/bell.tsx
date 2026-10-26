// The room's bell (#113, #115): what I'm notified of in this room (all
// messages, mentions only, nothing, or my default for rooms like it), and
// a snooze for everything. One menu, the app's own (`openMenu`), so it
// reads the same on the page and on a phone.

import type { ChatRoom } from "../proto.gen";
import { askText, openMenu, type MenuItem } from "../ui/menu";
import { LEVELS, levelLabel, loadNotify, notifyPrefs, parseSnooze, setNotify, snoozePresets, untilText } from "./notify";

/** Whether notifications are snoozed or in quiet hours now. */
export const paused = () => !!notifyPrefs()?.paused;

/** Open the bell's menu under `at`. `setLevel` does the choosing (the
 * store's); `toast` says what went wrong. */
export function openBell(
  e: { currentTarget: Element; preventDefault(): void },
  room: ChatRoom,
  setLevel: (level: ChatRoom["level"] | null) => Promise<unknown>,
  toast: (m: string) => void,
) {
  const fail = (err: Error) => toast(err.message);
  const p = notifyPrefs();
  // The channel's name as it's written, not in capitals (UI audit #169).
  const items: MenuItem[] = [room.kind === "channel" ? { header: `Notify me about #${room.name}`, name: true } : { header: "Notify me about this room" }];
  for (const l of LEVELS) {
    items.push({ label: l.label, hint: l.hint, checked: room.level === l.level, run: () => void setLevel(l.level).catch(fail) });
  }
  if (room.level_own) items.push({ label: "Use my default", hint: "Whatever Settings says for rooms like this", run: () => void setLevel(null).catch(fail) });
  items.push("separator");
  const snoozed = p?.snooze_until ? p.snooze_until > Date.now() : false;
  items.push({ header: snoozed && p?.snooze_until ? `Paused ${untilText(p.snooze_until)}` : p?.paused ? "Paused by your quiet hours" : "Pause notifications" });
  for (const s of snoozePresets()) items.push({ label: s.label, run: () => void setNotify({ snooze_until: s.until }).catch(fail) });
  items.push({
    label: "Until…",
    hint: "A time like 17:30 or 5pm, or a span like 45m or 3h",
    run: () =>
      void askText("Pause notifications until", "", "17:30, 5pm, 45m or 3h").then((v) => {
        if (v === null) return;
        const until = parseSnooze(v);
        if (until === null) return toast("A time like 17:30 or 5pm, or a span like 45m or 3h");
        return setNotify({ snooze_until: until }).catch(fail);
      }),
  });
  if (snoozed) items.push({ label: "Resume now", run: () => void setNotify({ snooze_until: 0 }).catch(fail) });
  const r = e.currentTarget.getBoundingClientRect();
  openMenu({ clientX: Math.max(8, r.right - 220), clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, items);
  // The first time, the menu opens on defaults and then fills in.
  if (!p) void loadNotify();
}

/** The bell's glyph and what it says: a moon while paused, a slashed bell
 * for "nothing", a dotted one for mentions only. */
export function bellFor(room: ChatRoom): { glyph: string; title: string } {
  const how = levelLabel(room.level);
  if (paused()) return { glyph: "🌙", title: `Notifications are paused. This room: ${how}` };
  return { glyph: room.level === "nothing" ? "🔕" : "🔔", title: `Notifications: ${how}` };
}
