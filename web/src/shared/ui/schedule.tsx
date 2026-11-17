// Picking when (Arugula #1101, Doc "Form controls: one kit", item 5): any of
// the seven days as a row of chips (`Weekdays`), and a from-to time as one
// field (`TimeRange`); and the week they make, drawn (`Week`, Arugula
// #1109). Settings' quiet hours use all three. The looks are
// ./schedule.css; the values ./days.ts.

import "./schedule.css";
import { clockOf, overnight, toggleDay, weekSpans, WEEKDAYS } from "./days";

type Data = { [data: `data-${string}`]: unknown };

/** Any of seven days, Monday first: each chip a toggle button. `value` is
 * the days on (1 Monday to 7 Sunday); `onChange` gets them in week order. */
export function Weekdays({
  value,
  onChange,
  label = "Days",
  disabled,
  ...attrs
}: { value: readonly number[]; onChange: (days: number[]) => void; label?: string; disabled?: boolean } & Data) {
  return (
    <span class="weekdays" role="group" aria-label={label} {...attrs}>
      {WEEKDAYS.map((d) => (
        <button
          key={d.day}
          type="button"
          title={d.name}
          aria-label={d.name}
          aria-pressed={value.includes(d.day)}
          data-day={d.day}
          disabled={disabled}
          onClick={() => onChange(toggleDay(value, d.day))}
        >
          {d.letter}
        </button>
      ))}
    </span>
  );
}

/** From and to, as "HH:MM", read as one field. A range that ends at or
 * before it starts runs into the next morning, and says so when `overnight`
 * is shown. */
export function TimeRange({
  from,
  to,
  onChange,
  label = "",
  showOvernight = true,
  disabled,
  ...attrs
}: {
  from: string;
  to: string;
  onChange: (range: { from: string; to: string }) => void;
  /** What the range is of, for the fields' names ("Weekdays" → "Weekdays from"). */
  label?: string;
  showOvernight?: boolean;
  disabled?: boolean;
} & Data) {
  const name = (end: string) => (label ? `${label} ${end}` : end[0].toUpperCase() + end.slice(1));
  return (
    <span class="time-range-wrap">
      <span class="time-range" data-disabled={disabled || undefined} {...attrs}>
        <input type="time" value={from} aria-label={name("from")} disabled={disabled} onInput={(e) => onChange({ from: e.currentTarget.value, to })} />
        <span class="time-range-to" aria-hidden="true">
          to
        </span>
        <input type="time" value={to} aria-label={name("to")} disabled={disabled} onInput={(e) => onChange({ from, to: e.currentTarget.value })} />
      </span>
      {showOvernight && overnight(from, to) && <span class="time-range-note">into the next morning</span>}
    </span>
  );
}

/** The week windows of days and times cover, drawn: a bar a day, Monday
 * first, midnight to midnight, with what's covered filled in. A window that
 * runs overnight fills the end of its day and the start of the next. */
export function Week({ windows, label = "Your week", ...attrs }: { windows: readonly { days: readonly number[]; from: string; to: string }[]; label?: string } & Data) {
  const week = weekSpans(windows);
  return (
    <div class="week" role="group" aria-label={label} {...attrs}>
      <div class="week-caption" aria-hidden="true">
        {label}
      </div>
      {WEEKDAYS.map((d, i) => (
        <div class="week-day" key={d.day} data-day={d.day}>
          <span class="week-name" aria-hidden="true">
            {d.name.slice(0, 3)}
          </span>
          <span
            class="week-bar"
            role="img"
            aria-label={`${d.name}: ${week[i].length ? week[i].map((s) => `${clockOf(s.from)} to ${clockOf(s.to)}`).join(", ") : "none"}`}
          >
            {week[i].map((s) => (
              <i key={s.from} data-from={clockOf(s.from)} data-to={clockOf(s.to)} style={{ left: `${(s.from / 1440) * 100}%`, width: `${((s.to - s.from) / 1440) * 100}%` }} />
            ))}
          </span>
        </div>
      ))}
      <div class="week-ticks" aria-hidden="true">
        <span>12a</span>
        <span>6a</span>
        <span>12p</span>
        <span>6p</span>
        <span>12a</span>
      </div>
    </div>
  );
}
