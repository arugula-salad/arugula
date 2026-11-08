// The card (#698): what a list or a page holds one of. `Card` draws the
// surface, a tone's bar, edge and fill, a dense one-line form and a `Bar`
// as its head; `.card-grid` and `.card-list` lay a set of them out. The
// looks are ./card.css, and its sizes and Latte's tint are tokens in
// shared/style.css.

import type { ComponentChildren, JSX } from "preact";
import type { Tone } from "./state-chip";
import "./card.css";

/** The class a card takes for its tone: `undefined` is a plain card, `null`
 * a toned one with no state to say (muted). */
export function cardToneClass(tone: Tone | null | undefined): string {
  return tone === undefined ? "" : tone === null ? "card-tone" : `card-tone card-${tone}`;
}

type Data = { [data: `data-${string}`]: unknown };

/** A card. `as` is its element (a `li` in a list, an `a` when the whole
 * card opens a page); `tone` gives it a bar, an edge and a fill; `dense`
 * makes it one line; `head` is a `Bar` (`sub`) for its first row, so the
 * card's padding moves to its body; `selected` (its drawer is open) and
 * `seen` (dealt with) are the states pages set on it. Anything else (`data-*`,
 * handlers, an `href`) lands on the element. */
export function Card({
  as = "div",
  tone,
  dense,
  head,
  selected,
  seen,
  class: cls,
  children,
  ...attrs
}: {
  as?: "div" | "li" | "a" | "article" | "section";
  tone?: Tone | null;
  dense?: boolean;
  head?: ComponentChildren;
  selected?: boolean;
  seen?: boolean;
  class?: string;
  children?: ComponentChildren;
} & Omit<JSX.IntrinsicElements["div"], "class" | "children" | "ref"> &
  Pick<JSX.IntrinsicElements["a"], "href" | "target" | "rel"> &
  Data) {
  const Tag = as as "div";
  const c = ["card", cardToneClass(tone), dense ? "card-dense" : "", head ? "has-bar" : "", seen ? "seen" : "", cls ?? ""].filter(Boolean).join(" ");
  return (
    <Tag class={c} aria-current={selected ? "true" : undefined} {...attrs}>
      {head ? (
        <>
          {head}
          <div class="card-body">{children}</div>
        </>
      ) : (
        children
      )}
    </Tag>
  );
}
