// Picking when (Arugula #1101, Doc "Form controls: one kit", item 5): any of
// the seven days as a row of chips (`Weekdays`), and a from-to time as one
// field (`TimeRange`). Settings' quiet hours use both. The looks are
// ./schedule.css; the values ./days.ts.

import "./schedule.css";
import { overnight, toggleDay, WEEKDAYS } from "./days";

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
