// Surface (kit §4.1, 1): a container. A section holds cards; a card is the
// tactile thing you act on; paper is a Doc's page; an overlay floats (a
// Sheet's body). Content inside is flat: rows, words, a well.

import type { ComponentChildren, JSX } from "preact";
import "./kit.css";

export type SurfaceKind = "section" | "card" | "paper" | "overlay";

type Data = { [data: `data-${string}`]: unknown };

/** A surface of `kind`. `as` is its element; `current` marks the one that's
 * open or picked (a ring, and `aria-current`). */
export function Surface({
  kind = "card",
  as = "div",
  current,
  class: cls,
  children,
  ...attrs
}: {
  kind?: SurfaceKind;
  as?: "div" | "section" | "article" | "li" | "aside";
  current?: boolean;
  class?: string;
  children?: ComponentChildren;
} & Omit<JSX.IntrinsicElements["div"], "class" | "children" | "ref"> &
  Data) {
  const Tag = as as "div";
  return (
    <Tag class={`k-surface is-${kind}${cls ? ` ${cls}` : ""}`} data-kit="surface" aria-current={current ? "true" : undefined} {...attrs}>
      {children}
    </Tag>
  );
}
