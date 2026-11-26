// The rail down the left (M108, #376): the brand, a button per place
// (Overview, Boards, Docs, Machines, Chat: Arugula #1345) and the foot. It's
// drawn here from its items, so a machine's own page's
// (shell/ui/shell/rail.tsx) and the app's are one component, with the same
// places. It knows nothing of either
// page's state: each builds its items and says which is lit.

import type { ComponentChildren } from "preact";
import { useRef } from "preact/hooks";
import { both, useBindings } from "../keys";

export type RailItem = {
  label: string;
  /** The button's title. */
  tip: string;
  /** Its accessible name, when it isn't the label (a count, say). */
  aria?: string;
  /** The `data-…` attribute that names it, for tests and the page's hooks. */
  hook: string;
  /** Lit: the place you're in. */
  here: boolean;
  icon: ComponentChildren;
  /** An unread count drawn on the corner. */
  count?: { n: number; mention: boolean };
  /** What a click does. With an `href`, a plain click only: a modified
   * one is the browser's (a new tab). */
  onClick?: (e: MouseEvent) => void;
  /** A link, for a place on another origin. */
  href?: string;
  target?: string;
};

/** Each place's `g` key (v6's), by its hook's name. */
const GO: Record<string, string> = { overview: "o", boards: "b", docs: "d", machines: "m", chat: "c", inbox: "i" };

/** `g o`, `g b`…: the rail's places by key, as a click on each (the
 * items as last drawn). */
function useGoKeys(items: RailItem[]) {
  const latest = useRef(items);
  latest.current = items;
  const places = items.map((i) => i.hook.replace(/^data-open-/, "")).filter((p) => GO[p]);
  useBindings(
    places.map((place) => ({
      id: `go.${place}`,
      label: items.find((i) => i.hook === `data-open-${place}`)!.label,
      group: "go" as const,
      keys: both(`key:g key:${GO[place]}`),
      run: () => {
        const item = latest.current.find((i) => i.hook === `data-open-${place}`);
        if (item?.onClick) item.onClick(new MouseEvent("click"));
        else if (item?.href) location.assign(item.href);
      },
    })),
    [places.join()],
  );
}

export function Rail({ brand, items, bottom, foot, label = "Places" }: { brand: ComponentChildren; items: RailItem[]; bottom: RailItem[]; foot?: ComponentChildren; label?: string }) {
  useGoKeys([...items, ...bottom]);
  return (
    <nav class="ws-rail" aria-label={label}>
      {brand}
      {items.map((item) => (
        <RailButton key={item.hook} item={item} />
      ))}
      <div class="ws-rail-fill" />
      {bottom.map((item) => (
        <RailButton key={item.hook} item={item} />
      ))}
      {foot}
    </nav>
  );
}

export function RailButton({ item }: { item: RailItem }) {
  const { label, aria, tip, here, hook, count, onClick, href, target, icon } = item;
  const inner = (
    <>
      <span class="ws-rail-pill">{icon}</span>
      {count && count.n > 0 && <span class={count.mention ? "ws-rail-count mention" : "ws-rail-count"}>{count.mention ? `@${count.n}` : count.n}</span>}
      <span class="ws-rail-label" aria-hidden="true">
        {label}
      </span>
    </>
  );
  const props = {
    class: `ws-rail-button${here ? " here" : ""}`,
    "aria-label": aria ?? label,
    "aria-current": here ? ("page" as const) : undefined,
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
    <button {...props} onClick={onClick}>
      {inner}
    </button>
  );
}
