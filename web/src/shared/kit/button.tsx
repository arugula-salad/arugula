// Button (kit §4.1, 13): primary (one to a place), plain, or danger; a key
// that does the same shows as a Kbd inside it, and is its aria-keyshortcuts.

import type { ComponentChildren, JSX } from "preact";
import { Kbd } from "./kbd";
import "./kit.css";

export type ButtonKind = "primary" | "plain" | "danger";

export function Button({
  kind = "plain",
  keys,
  class: cls,
  children,
  ...attrs
}: { kind?: ButtonKind; keys?: string; class?: string; children: ComponentChildren } & Omit<JSX.IntrinsicElements["button"], "class" | "children">) {
  return (
    <button type="button" class={`k-button is-${kind}${cls ? ` ${cls}` : ""}`} data-kit="button" aria-keyshortcuts={keys} {...attrs}>
      {children}
      {keys ? (
        <span aria-hidden="true">
          <Kbd keys={keys} />
        </span>
      ) : null}
    </button>
  );
}
