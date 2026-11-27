// Field (kit §4.1, 14): a text box, a toggle that takes effect now, and a
// choice of one of two to four. Each carries its label, so a field is never
// a box alone.

import "./kit.css";

export function TextField({
  label,
  value,
  onInput,
  onEnter,
  placeholder,
  hint,
  warn,
  disabled,
}: {
  label: string;
  value: string;
  onInput: (v: string) => void;
  onEnter?: () => void;
  placeholder?: string;
  /** A word or two after the box: "no secrets". */
  hint?: string;
  /** What's wrong with what's typed, under the box ("That looks like a
   * key: leave it out"): the box's edge says so too. */
  warn?: string;
  disabled?: boolean;
}) {
  return (
    <label class="k-field" data-kit="field" data-warn={warn ? "" : undefined}>
      {label}
      <span class="k-field-box">
        <input
          value={value}
          placeholder={placeholder}
          onInput={(e) => onInput(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && onEnter) {
              e.preventDefault();
              onEnter();
            }
          }}
          disabled={disabled}
          aria-invalid={warn ? true : undefined}
        />
        {hint ? <span class="k-field-hint m-meta">{hint}</span> : null}
      </span>
      {warn ? (
        <span class="k-field-warn" role="status">
          {warn}
        </span>
      ) : null}
    </label>
  );
}

export function Toggle({
  label,
  on,
  onChange,
}: {
  label: string;
  on: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <button
      type="button"
      class="k-toggle"
      role="switch"
      aria-checked={on}
      data-kit="toggle"
      onClick={() => onChange(!on)}
    >
      <span class="k-toggle-track" aria-hidden="true" />
      {label}
    </button>
  );
}

/** One of a few, as segments: a radio group the arrow keys move through. */
export function Choice<T extends string>({
  label,
  options,
  value,
  onChange,
}: {
  label: string;
  options: readonly { id: T; label: string }[];
  value: T;
  onChange: (v: T) => void;
}) {
  const move = (e: KeyboardEvent) => {
    const step =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (!step) return;
    e.preventDefault();
    const i =
      (options.findIndex((o) => o.id === value) + step + options.length) %
      options.length;
    onChange(options[i]!.id);
    (
      (e.currentTarget as HTMLElement).children[i] as HTMLElement | undefined
    )?.focus();
  };
  return (
    <div
      class="k-choice"
      role="radiogroup"
      aria-label={label}
      data-kit="choice"
      onKeyDown={move}
    >
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          class="k-choice-option"
          role="radio"
          aria-checked={o.id === value}
          tabIndex={o.id === value ? 0 : -1}
          onClick={() => onChange(o.id)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
