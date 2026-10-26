// How loud chat is and when to be left alone (#113, #115), on /settings:
// the level a channel or DM gets when you haven't set one for it, a pause
// (a snooze now, or quiet hours by weekday in your time zone), and whether
// the pause covers every push or only chat's. A room's own level is its
// bell's (shared/chat/bell). The platform's (`/api/notify/prefs`). Quiet
// hours are a section of their own, in the index (UI audit #168).

import { useEffect, useState } from "preact/hooks";
import type { NotifyLevel, QuietHours } from "../../shared/proto.gen";
import { clock, deviceZone, LEVELS, loadNotify, minutes, notifyPrefs, setNotify, snoozePresets, subscribeNotify, untilText } from "../../shared/chat/notify";
import { useSubscribe } from "../../shared/ui/hooks";
import { Section } from "./section";

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

type Day = { on: boolean; from: string; to: string };

/** The schedule's windows as one row per weekday (the last one covering a
 * day wins; the page writes one window per day-pair of times). */
function rowsOf(quiet: QuietHours[]): Day[] {
  const rows: Day[] = DAYS.map(() => ({ on: false, from: "22:00", to: "08:00" }));
  for (const q of quiet) for (const d of q.days) rows[d - 1] = { on: true, from: clock(q.from), to: clock(q.to) };
  return rows;
}

/** Days with the same times share a window. */
function windowsOf(rows: Day[]): QuietHours[] {
  const by = new Map<string, QuietHours>();
  rows.forEach((r, i) => {
    if (!r.on) return;
    const key = `${r.from}-${r.to}`;
    const w = by.get(key) ?? { days: [], from: minutes(r.from), to: minutes(r.to) };
    w.days.push(i + 1);
    by.set(key, w);
  });
  return [...by.values()];
}

const zones = (): string[] => {
  try {
    return (Intl as unknown as { supportedValuesOf(k: string): string[] }).supportedValuesOf("timeZone");
  } catch {
    return [];
  }
};

export function NotificationSettings() {
  useSubscribe(subscribeNotify);
  useEffect(() => void loadNotify(), []);
  const p = notifyPrefs();
  const [rows, setRows] = useState<Day[] | null>(null);
  const [tz, setTz] = useState<string | null>(null);
  const [err, setErr] = useState("");
  if (!p) return null;
  const mine = rows ?? rowsOf(p.quiet);
  const zone = tz ?? p.tz;
  const run = (patch: Parameters<typeof setNotify>[0]) => setNotify(patch).then(() => setErr(""), (e: Error) => setErr(e.message));
  const level = (key: "channels" | "dms" | "threads", label: string) => (
    <li>
      <label class="settings-field">
        {label}
        <select class="field" value={p[key]} data-notify-default={key} onChange={(e) => void run({ [key]: e.currentTarget.value as NotifyLevel })}>
          {LEVELS.map((l) => (
            <option key={l.level} value={l.level}>
              {l.label}
            </option>
          ))}
        </select>
      </label>
    </li>
  );
  const snoozed = !!p.snooze_until;
  return (
    <>
      <Section
        id="notifications"
        title="Chat notifications"
        lede="How much of chat pushes you, and when to leave you alone."
        hooks={{ "data-notify-settings": "" }}
        more={
          <p>
            Each room's bell overrides the defaults here, except that a thread you follow goes by its own line: its replies push you unless you chose "Mentions only" (a mention in it still does) or silenced the room. "Mentions only" still pushes a DM. Pausing holds back pushes only: unread counts and the
            moon others see on your avatar carry on.
          </p>
        }
      >
        <ul class="platform-list settings-rows">
          {level("channels", "Channels")}
          {level("dms", "Direct messages")}
          {level("threads", "Threads you follow")}
          <li data-notify-pause>
            <strong>{p.paused ? (snoozed && p.snooze_until ? `Paused ${untilText(p.snooze_until)}` : "Paused by your quiet hours") : "Not paused"}</strong>
            {/* What the buttons do, before them (UI audit #168). */}
            <span class="dim">{snoozed ? "Pause instead for" : "Pause for"}</span>
            {snoozePresets().map((s) => (
              <button key={s.label} class="secondary small" data-notify-snooze={s.label} onClick={() => void run({ snooze_until: s.until })}>
                {s.label}
              </button>
            ))}
            {snoozed && (
              <button class="secondary small" data-notify-resume onClick={() => void run({ snooze_until: 0 })}>
                Resume now
              </button>
            )}
          </li>
          <li>
            <label class="settings-switch">
              <input
                type="checkbox"
                role="switch"
                class="sw-switch"
                checked={p.all_notices}
                data-notify-all
                onChange={(e) => void run({ all_notices: e.currentTarget.checked })}
              />
              <span>Pausing covers every notice, not just chat's</span>
            </label>
            <span class="dim">Inbox items and deploy approvals wait too. Alerts about your account never do.</span>
          </li>
        </ul>
      </Section>
      <Section
        id="quiet-hours"
        title="Quiet hours"
        lede="Days and hours chat leaves you alone, in the time zone here. Tick a day to set its hours."
        more={<p>They follow the time zone chosen here, not the device's, so travelling doesn't move them.</p>}
        hooks={{ "data-notify-quiet": "" }}
      >
        <ul class="platform-list settings-rows notify-schedule" data-notify-schedule>
          <li>
            <label class="settings-field">
              Time zone
              <input class="field" list="notify-zones" value={zone} data-notify-tz onInput={(e) => setTz(e.currentTarget.value)} />
            </label>
            <datalist id="notify-zones">
              {zones().map((z) => (
                <option key={z} value={z} />
              ))}
            </datalist>
            {zone !== deviceZone() && (
              <button class="link small" onClick={() => setTz(deviceZone())}>
                Use this device's ({deviceZone()})
              </button>
            )}
          </li>
          {DAYS.map((d, i) => (
            <li key={d} data-notify-day={d}>
              <label class="settings-switch notify-day">
                <input
                  type="checkbox"
                  role="switch"
                  class="sw-switch"
                  checked={mine[i].on}
                  onChange={(e) => setRows(mine.map((r, j) => (j === i ? { ...r, on: e.currentTarget.checked } : r)))}
                />
                <span>{d}</span>
              </label>
              <input
                type="time"
                class="field small"
                value={mine[i].from}
                disabled={!mine[i].on}
                aria-label={`${d} from`}
                onInput={(e) => setRows(mine.map((r, j) => (j === i ? { ...r, from: e.currentTarget.value } : r)))}
              />
              <span class="dim">to</span>
              <input
                type="time"
                class="field small"
                value={mine[i].to}
                disabled={!mine[i].on}
                aria-label={`${d} to`}
                onInput={(e) => setRows(mine.map((r, j) => (j === i ? { ...r, to: e.currentTarget.value } : r)))}
              />
              {mine[i].on && minutes(mine[i].to) <= minutes(mine[i].from) && <span class="dim">into the next morning</span>}
            </li>
          ))}
          <li>
            <button
              class="primary"
              data-notify-save
              disabled={rows === null && tz === null}
              onClick={() => void run({ tz: zone, quiet: windowsOf(mine) }).then(() => (setRows(null), setTz(null)))}
            >
              Save quiet hours
            </button>
            {err && <span class="error">{err}</span>}
          </li>
        </ul>
      </Section>
    </>
  );
}
