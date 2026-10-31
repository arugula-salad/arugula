// One chip (UI audit #183): a state, a label or a badge, in one look with
// tone variants, on the platform page and on control's. A state (a run's,
// a deploy's, an issue's, a workspace's) is a `StateChip`: the chip with a
// dot, in the tone its card's bar takes in a list (../states.ts), its word
// capitalised wherever it shows. A label is a muted chip and an agent's
// badge a teal one, without the dot. The look is ./state-chip.css.

import type { ComponentChildren } from "preact";
import "./state-chip.css";

/** A chip's tone, by the palette color it takes (as a card's bar does,
 * platform/swarm.css). */
export type Tone = "accent" | "ok" | "warn" | "danger" | "info" | "teal" | "merged" | "muted";

type Data = { [data: `data-${string}`]: unknown };

/** A chip: `tone` colors it (muted when none), `dot` marks a state. With
 * `onClick` it's a button. Anything else (`data-*`, a title) lands on it. */
export function Chip({
  tone,
  dot,
  class: cls,
  title,
  onClick,
  children,
  ...attrs
}: {
  tone?: Tone | null;
  dot?: boolean;
  class?: string;
  title?: string;
  onClick?: (e: MouseEvent) => void;
  children: ComponentChildren;
} & Data) {
  const c = `chip chip-${tone ?? "muted"}${dot ? " chip-dot" : ""}${cls ? ` ${cls}` : ""}`;
  return onClick ? (
    <button type="button" class={c} title={title} onClick={onClick} {...attrs}>
      {children}
    </button>
  ) : (
    <span class={c} title={title} {...attrs}>
      {children}
    </span>
  );
}

/** A state, as one chip everywhere: a dot and its word in its tone. */
export function StateChip({ tone, children, ...rest }: { tone: Tone | null; class?: string; title?: string; children: ComponentChildren } & Data) {
  return (
    <Chip tone={tone} dot {...rest}>
      {children}
    </Chip>
  );
}
