// How loud chat is and when to be left alone (#113, #115), parts of
// Settings › You's Notifications (Arugula #1320): the level a channel or
// DM gets when you haven't set one for it, a pause (a snooze now, or quiet
// hours by weekday in your time zone), and whether the pause covers every
// push or only chat's. A room's own level is its bell's (shared/chat/bell).
// The platform's (`/api/notify/prefs`). Quiet hours are a part of their
// own, after the words that notify you: the nights, one window and the
// week they make, drawn (Arugula #1109).

import { useEffect, useState } from "preact/hooks";
import type { NotifyLevel, QuietHours } from "../../shared/proto.gen";
import { clock, deviceZone, LEVELS, levelLabel, loadNotify, minutes, notifyPrefs, setNotify, snoozePresets, subscribeNotify, untilText } from "../../shared/chat/notify";
import { claimDays, sameTimes } from "../../shared/ui/days";
import { useSubscribe } from "../../shared/ui/hooks";
import { TimeRange, Week, Weekdays } from "../../shared/ui/schedule";
import { Section } from "../../shared/ui/settings-section";
import { joined, quietSummary } from "./summary";

type Day = { on: boolean; from: string; to: string };

/** The schedule's windows as one row per weekday (the last one covering a
 * day wins; the page writes one window per day-pair of times). */
function rowsOf(quiet: QuietHours[]): Day[] {
  const rows: Day[] = Array.from({ length: 7 }, () => ({ on: false, from: "22:00", to: "08:00" }));
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

/** A window as the page edits it: its days (chips) and its times (one range). */
type QuietWindow = { days: number[]; from: string; to: string };

/** The schedule as windows that don't share a day, each with its own days
 * and times (Arugula #1101); with none saved, one with no days yet. */
function editOf(quiet: QuietHours[]): QuietWindow[] {
  const wins = windowsOf(rowsOf(quiet)).map((w) => ({ days: w.days, from: clock(w.from), to: clock(w.to) }));
  return wins.length ? wins : [{ days: [], from: "22:00", to: "08:00" }];
}

/** What's saved: the windows with a day in them. */
function savedOf(wins: QuietWindow[]): QuietHours[] {
  return wins.filter((w) => w.days.length).map((w) => ({ days: w.days, from: minutes(w.from), to: minutes(w.to) }));
}

const zones = (): string[] => {
  try {
    return (Intl as unknown as { supportedValuesOf(k: string): string[] }).supportedValuesOf("timeZone");
  } catch {
    return [];
  }
};

/** The prefs once loaded, and a change to them that says why it failed. */
function useNotify() {
  useSubscribe(subscribeNotify);
  useEffect(() => void loadNotify(), []);
  const [err, setErr] = useState("");
  const run = (patch: Parameters<typeof setNotify>[0]) => setNotify(patch).then(() => setErr(""), (e: Error) => setErr(e.message));
  return { p: notifyPrefs(), run, err };
}

/** How loud chat is: each kind of room's level, and a pause. */
export function ChatNotifications() {
  const { p, run, err } = useNotify();
  if (!p) return null;
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
    <Section
      id="chat-notify"
      title="Chat"
      summary={joined(levelLabel(p.channels), p.paused ? (snoozed && p.snooze_until ? `paused ${untilText(p.snooze_until)}` : "paused by quiet hours") : "not paused")}
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
            <span title="Inbox items and deploy approvals wait too. Alerts about your account never do.">Pause everything, not just chat</span>
          </label>
        </li>
      </ul>
      {err && <p class="control-error">{err}</p>}
    </Section>
  );
}

/** Quiet hours: the nights, their hours and the time zone they're in. */
export function QuietHoursSettings() {
  const { p, run, err } = useNotify();
  const [wins, setWins] = useState<QuietWindow[] | null>(null);
  const [tz, setTz] = useState<string | null>(null);
  if (!p) return null;
  const mine = wins ?? editOf(p.quiet);
  // Until quiet hours are saved the zone is the platform's default (UTC);
  // offer this device's instead (UI audit #200).
  const zone = tz ?? (p.quiet.length === 0 && p.tz === "UTC" ? deviceZone() : p.tz);
  return (
    <Section
      id="quiet-hours"
      title="Quiet hours"
      summary={quietSummary(p.quiet, p.tz)}
      more={<p>They follow the time zone chosen here, not the device's, so travelling doesn't move them.</p>}
      hooks={{ "data-notify-quiet": "" }}
    >
      <ul class="platform-list settings-rows notify-schedule" data-notify-schedule>
        {/* The nights as chips and their hours as one range (Arugula #1101);
            different times on some days is a window of their own, and a
            day is in one window, so picking it in another moves it there. */}
        {mine.map((w, i) => (
          <li key={i} data-notify-window={i}>
            <Weekdays
              value={w.days}
              label={i === 0 ? "Quiet days" : "Quiet days, other hours"}
              onChange={(days) => setWins(days.length || i === 0 ? claimDays(mine, i, days) : mine.filter((_, j) => j !== i))}
            />
            <TimeRange
              from={w.from}
              to={w.to}
              label={i === 0 ? "Quiet hours" : "Other quiet hours"}
              onChange={(r) => setWins(mine.map((x, j) => (j === i ? { ...x, ...r } : x)))}
            />
          </li>
        ))}
        {/* One window is the usual week; the rest wait behind a link
            (Arugula #1109, the Settings Doc's decision 5). */}
        {(mine.length > 1 || mine[0].days.length > 0) && (
          <li class="notify-split">
            {mine.length < 7 && mine.every((w) => w.days.length) && (
              <button class="link small" data-notify-add-window onClick={() => setWins([...mine, { days: [], from: mine[0].from, to: mine[0].to }])}>
                {mine.length === 1 ? "Different times on some days…" : "Different times on more days…"}
              </button>
            )}
            {mine.length > 1 && (
              <button class="link small" data-notify-same-times onClick={() => setWins(sameTimes(mine))}>
                Same times every night
              </button>
            )}
          </li>
        )}
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
        <li>
          <Week windows={mine} data-notify-week />
        </li>
        <li>
          <button
            class="primary"
            data-notify-save
            disabled={wins === null && tz === null}
            onClick={() => void run({ tz: zone, quiet: savedOf(mine) }).then(() => (setWins(null), setTz(null)))}
          >
            Save quiet hours
          </button>
          {err && <span class="error">{err}</span>}
        </li>
      </ul>
    </Section>
  );
}
