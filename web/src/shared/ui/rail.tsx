// The rail down the left (M108, #376): the mark, the places and Settings.
// It's drawn here from its items, so the panes page's (shell/ui/shell/rail.tsx)
// and the platform page's are one component. It knows nothing of either
// page's state: each builds its items and says which is lit.

import type { ComponentChildren } from "preact";

export type RailItem = {
  label: string;
  /** The button's title. */
  tip: string;
  /** Its accessible name, when it isn't the label (a count, say). */
  aria?: string;
  /** The `data-…` attribute that names it, for tests and the page's hooks. */
  hook: string;
  /** Lit: the place you're in (or, for a `toggle`, that it's on). */
  here: boolean;
  icon: ComponentChildren;
  /** An unread count drawn on the corner. */
  count?: { n: number; mention: boolean };
  /** A switch (the sidebar), not a place: drawn as on, not as here, and in
   * the group under the mark. */
  toggle?: boolean;
  /** The id of what a `toggle` shows and hides. */
  controls?: string;
  /** What a click does. With an `href`, a plain click only: a modified
   * one is the browser's (a new tab). */
  onClick?: (e: MouseEvent) => void;
  /** A link, for a place on another origin. */
  href?: string;
  target?: string;
};

export function Rail({ brand, items, bottom, label = "Places" }: { brand: ComponentChildren; items: RailItem[]; bottom: RailItem[]; label?: string }) {
  return (
    <nav class="ws-rail" aria-label={label}>
      {brand}
      {items.map((item) => (item.toggle ? <div class="ws-rail-utility" key={item.hook}><RailButton item={item} /></div> : <RailButton key={item.hook} item={item} />))}
      <div class="ws-rail-fill" />
      {bottom.map((item) => (
        <RailButton key={item.hook} item={item} />
      ))}
    </nav>
  );
}

export function RailButton({ item }: { item: RailItem }) {
  const { label, aria, tip, here, toggle, controls, hook, count, onClick, href, target, icon } = item;
  const inner = (
    <>
      {icon}
      {count && count.n > 0 && <span class={count.mention ? "ws-rail-count mention" : "ws-rail-count"}>{count.mention ? `@${count.n}` : count.n}</span>}
      {toggle && <span class="ws-sidebar-chevron" aria-hidden="true">{here ? "‹" : "›"}</span>}
      <span class="ws-rail-label" aria-hidden="true">
        {label}
      </span>
    </>
  );
  const props = {
    class: `ws-rail-button${here ? (toggle ? " on" : " here") : ""}`,
    "aria-label": aria ?? label,
    "aria-current": !toggle && here ? ("page" as const) : undefined,
    title: tip,
    [hook]: "",
  };
  if (href)
    return (
      <a
        {...props}
        href={href}
        target={target}
        onClick={(e) => {
          if (!onClick || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
          e.preventDefault();
          onClick(e);
        }}
      >
        {inner}
      </a>
    );
  return (
    <button {...props} aria-expanded={toggle ? here : undefined} aria-controls={toggle ? controls : undefined} onClick={onClick}>
      {inner}
    </button>
  );
}
