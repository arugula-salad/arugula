// The bar (#686): the one row above a pane, a block, a chat room and the
// phone. It is 34px (44 on the phone), never wraps, and what doesn't fit
// goes into ⋯. A pane's header (`PaneHeader`) draws through it, so do a
// block's second bar (`sub`) and a chat room's head.
//
// Slots, left to right: `lead` (an ordinal, a glyph), `title`, `where`
// (dim, inline), `status`, then `children` (a block's own controls, free
// to shrink), then `fold` and `actions` at the right. `fold` is what may
// leave the row for ⋯, the least important first: it goes when the bar is
// narrower than `foldBelow`, or one at a time while the row overflows.
// `actions` stay.

import { Fragment, type ComponentChildren, type JSX } from "preact";
import { useLayoutEffect, useRef, useState } from "preact/hooks";
import { openMenu, type MenuItem } from "./menu";
import "./bar.css";

export interface BarFold {
  key: string;
  /** What draws in the row. */
  node: ComponentChildren;
  /** What it is in ⋯. */
  item: MenuItem;
}

type DivProps = Omit<JSX.IntrinsicElements["div"], "title" | "class" | "children" | "ref">;

export function Bar({
  lead,
  title,
  where,
  status,
  actions,
  fold,
  foldBelow,
  sub,
  tip,
  class: cls,
  children,
  ...rest
}: {
  lead?: ComponentChildren;
  /** A `BarTitle`. */
  title?: ComponentChildren;
  /** A `BarWhere`. */
  where?: ComponentChildren;
  status?: ComponentChildren;
  /** Always in the row, last. */
  actions?: ComponentChildren;
  /** May go into ⋯, the least important first. */
  fold?: BarFold[];
  /** Fold all of them when the bar is narrower than this many px. */
  foldBelow?: number;
  /** A block's second bar, under the pane's. */
  sub?: boolean;
  /** The whole bar's tooltip. */
  tip?: string;
  class?: string;
  children?: ComponentChildren;
} & DivProps) {
  const row = useRef<HTMLDivElement>(null);
  const [folded, setFolded] = useState(0);
  const [narrow, setNarrow] = useState(false);
  const count = fold?.length ?? 0;

  useLayoutEffect(() => {
    const el = row.current;
    if (!el || count === 0) return;
    let width = el.clientWidth;
    const below = (w: number) => foldBelow !== undefined && w < foldBelow;
    setNarrow(below(width));
    const ro = new ResizeObserver(() => {
      if (el.clientWidth === width) return;
      width = el.clientWidth;
      setNarrow(below(width));
      setFolded(0);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [count, foldBelow]);

  // While the row overflows, fold one more.
  useLayoutEffect(() => {
    const el = row.current;
    if (!el || narrow || folded >= count) return;
    if (el.scrollWidth > el.clientWidth) setFolded(folded + 1);
  });

  const out = narrow ? count : Math.min(folded, count);
  const shown = fold?.slice(out) ?? [];
  const gone = fold?.slice(0, out) ?? [];
  const more = (e: MouseEvent) => openMenu(e, gone.map((f) => f.item), e.currentTarget as HTMLElement);

  return (
    <div ref={row} title={tip} class={`hbar${sub ? " sub" : ""}${cls ? ` ${cls}` : ""}`} {...rest}>
      {lead}
      {title}
      {where}
      {status}
      {children}
      <span class="hbar-fill" />
      {shown.map((f) => (
        <Fragment key={f.key}>{f.node}</Fragment>
      ))}
      {actions}
      {gone.length > 0 && (
        <button type="button" class="bar-btn sq hbar-more" aria-label="More" title="More" aria-haspopup="menu" data-hbar-more onClick={more}>
          ⋯
        </button>
      )}
    </div>
  );
}

type SpanProps = Omit<JSX.IntrinsicElements["span"], "ref">;

/** The bar's title: 13/500, cut short last. */
export const BarTitle = ({ class: cls, ...rest }: SpanProps) => <span class={cls ? `hbar-title ${cls}` : "hbar-title"} {...rest} />;

/** What the title is in, dim and inline, cut short first. */
export const BarWhere = ({ class: cls, ...rest }: SpanProps) => <span class={cls ? `hbar-where ${cls}` : "hbar-where"} {...rest} />;
