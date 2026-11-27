// Tabs and panes (design lift §4.1, part 11; Arugula #1483): a pane's one
// title row, over the bar (`../../ui/bar.tsx`, the one row above a pane, a
// block and a room). A place's pane (`./panes.tsx`) and a machine's pane
// (the shell's `PaneHeader`) both draw through it: its kind, its title, where
// it is or who opened it, its state, and its controls at the right. A
// pane's own slots (an ordinal, a branch, a host) go in `lead`, `status`
// and `children`; its class keeps each place's look until Lift 6a joins them.

import type { ComponentChildren, JSX } from "preact";
import { Bar, BarTitle, BarWhere } from "../../ui/bar";
import { StateMark, type State } from "../state-mark";

type DivProps = Omit<JSX.IntrinsicElements["div"], "title" | "class" | "children" | "ref">;

export function PaneHead({
  icon,
  lead,
  title,
  meta,
  where,
  state,
  dot,
  status,
  actions,
  tip,
  class: cls = "k-pane-head",
  children,
  ...rest
}: {
  /** Its kind, as a glyph. */
  icon?: ComponentChildren;
  /** Before the title, instead of `icon` (a machine's pane: its ordinal and glyph). */
  lead?: ComponentChildren;
  /** Its title: words, or a `BarTitle` of the pane's own; none for a place's default pane, which has its own heading. */
  title?: ComponentChildren;
  /** Dim after the title: who opened it, what it stands in place of. */
  meta?: ComponentChildren;
  /** Instead of `meta`, a `BarWhere` of the pane's own. */
  where?: ComponentChildren;
  /** Its state, as a mark and its word. */
  state?: { state: State; word?: string };
  /** New since you looked. */
  dot?: boolean;
  /** Instead of `state`: the pane's own status. */
  status?: ComponentChildren;
  /** Its controls, at the right. */
  actions?: ComponentChildren;
  tip?: string;
  class?: string;
  children?: ComponentChildren;
} & DivProps) {
  return (
    <Bar
      class={cls}
      tip={tip}
      lead={lead ?? icon}
      title={typeof title === "string" ? <BarTitle class="k-pane-title">{title}</BarTitle> : (title ?? undefined)}
      where={where ?? (meta ? <BarWhere class="k-pane-meta">{meta}</BarWhere> : undefined)}
      status={
        status ?? (
          <>
            {dot && <span class="k-pane-dot" aria-label="New" data-pane-dot />}
            {state && <StateMark state={state.state} word={state.word} />}
          </>
        )
      }
      actions={actions}
      {...rest}
    >
      {children}
    </Bar>
  );
}
