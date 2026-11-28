// Row (kit §4.1, 2): one flat line, mark · kind · title · who and meta ·
// age, on a section or a card (never a card of its own). Plain, a need
// (a thin amber tick at its left, never a fill) or selected. A row that
// expands holds its body under the line, and its line says so
// (`aria-expanded`); a NeedRow is one. A row nothing clicks (an ask that is
// always open, in an agent block or beside a terminal) has a plain line,
// not a button that does nothing. `lead` goes before the mark: a list's
// select box. One that `keep`s its body draws it while closed, hidden, so
// what the body reads stays current (a Settings row's summary, read off
// its controls). A row that's a place (a chat room) is a link, `href`, so a
// middle click opens it elsewhere; `line` adds to the line itself (its
// title, a drag, a menu), and `bold` says it has something unread.

import type { ComponentChildren, JSX } from "preact";
import { Mark, type State } from "./state-mark";
import "./kit.css";

export type RowKind = "plain" | "need" | "selected" | "done";

export function Row({
  kind = "plain",
  lead,
  mark,
  label,
  labelTip,
  title,
  meta,
  open,
  keep,
  onClick,
  href,
  line: lineAttrs,
  bold,
  inert,
  id,
  children,
  ...data
}: {
  inert?: boolean;
  /** Draw the body while closed, hidden. */
  keep?: boolean;
  id?: string;
  kind?: RowKind;
  /** Before the mark: a control of the row's own (its select box). */
  lead?: ComponentChildren;
  mark?: State;
  /** The kind of thing, a word: "ask", "board", "doc". */
  label?: string;
  /** The label's tooltip: the whole of a label cut short. */
  labelTip?: string;
  title: ComponentChildren;
  /** Who and when, after the title, each dim (or as given). */
  meta?: ComponentChildren[];
  /** Expanded: `children` show under the line. Undefined: it doesn't expand. */
  open?: boolean;
  onClick?: (e: MouseEvent) => void;
  /** A link: the line is an `<a>`; `onClick` takes a plain click. */
  href?: string;
  /** More on the line: its title, a drag, a menu. */
  line?: Pick<JSX.IntrinsicElements["a"], "title" | "draggable" | "onDragStart" | "onDragEnd" | "onContextMenu">;
  /** Something unread: its title is heavier. */
  bold?: boolean;
  children?: ComponentChildren;
  [data: `data-${string}`]: unknown;
}) {
  const line = (
    <>
        {lead}
        {mark ? <Mark state={mark} /> : null}
        {label ? <span class="k-row-kind m-meta" title={labelTip}>
            {label}
          </span> : null}
        <span class="k-row-title" data-row-title>
          {title}
        </span>
        {meta?.map((m, i) => (
          <span key={i} class="k-row-meta m-meta">
            {m}
          </span>
        ))}
    </>
  );
  return (
    <div id={id} class={`k-row is-${kind}`} data-kit="row" data-open={open || undefined} data-bold={bold || undefined} inert={inert} {...data}>
      {href !== undefined ? (
        <a {...lineAttrs} href={href} class="k-row-line" aria-current={kind === "selected" ? "page" : undefined} onClick={onClick}>
          {line}
        </a>
      ) : onClick ? (
        <button {...(lineAttrs as JSX.IntrinsicElements["button"])} type="button" class="k-row-line" aria-expanded={open} aria-current={kind === "selected" ? "true" : undefined} onClick={onClick}>
          {line}
        </button>
      ) : (
        <div class="k-row-line" aria-current={kind === "selected" ? "true" : undefined}>
          {line}
        </div>
      )}
      {(open || keep) && children ? (
        <div class="k-row-body" hidden={!open}>
          {children}
        </div>
      ) : null}
    </div>
  );
}
