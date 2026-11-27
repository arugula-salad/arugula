// Row (kit §4.1, 2): one flat line, mark · kind · title · who and meta ·
// age, on a section or a card (never a card of its own). Plain, a need
// (a thin amber tick at its left, never a fill) or selected. A row that
// expands holds its body under the line, and its line says so
// (`aria-expanded`); a NeedRow is one.

import type { ComponentChildren } from "preact";
import { Mark, type State } from "./state-mark";
import "./kit.css";

export type RowKind = "plain" | "need" | "selected" | "done";

export function Row({
  kind = "plain",
  mark,
  label,
  title,
  meta,
  open,
  onClick,
  inert,
  children,
  ...data
}: {
  inert?: boolean;
  kind?: RowKind;
  mark?: State;
  /** The kind of thing, a word: "ask", "board", "doc". */
  label?: string;
  title: ComponentChildren;
  /** Who and when, after the title, each dim (or as given). */
  meta?: ComponentChildren[];
  /** Expanded: `children` show under the line. Undefined: it doesn't expand. */
  open?: boolean;
  onClick?: () => void;
  children?: ComponentChildren;
  [data: `data-${string}`]: unknown;
}) {
  return (
    <div class={`k-row is-${kind}`} data-kit="row" data-open={open || undefined} inert={inert} {...data}>
      <button type="button" class="k-row-line" aria-expanded={open} aria-current={kind === "selected" ? "true" : undefined} onClick={onClick}>
        {mark ? <Mark state={mark} /> : null}
        {label ? <span class="k-row-kind m-meta">{label}</span> : null}
        <span class="k-row-title" data-row-title>
          {title}
        </span>
        {meta?.map((m, i) => (
          <span key={i} class="k-row-meta m-meta">
            {m}
          </span>
        ))}
      </button>
      {open && children ? <div class="k-row-body">{children}</div> : null}
    </div>
  );
}
