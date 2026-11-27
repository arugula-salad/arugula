// Tabs (kit §4.1, 11, the tabs half; the panes are ./panes/): an icon, a
// state mark, a title, a count, a dot (new, or waiting on you) and a close.
// The arrow keys, Home and End move between them; Delete closes the focused
// one; F2 renames one that can be (in place: Enter keeps it, Esc doesn't).
// A place's own (a machine's tabs, Lift 6a) puts its drag, its menu and its
// badges on each tab, and its + after them.

import type { ComponentChildren, JSX, Ref } from "preact";
import { CloseIcon } from "../ui/icons";
import { Mark, type State } from "./state-mark";
import "./kit.css";

export interface TabItem {
  id: string;
  title: string;
  icon?: ComponentChildren;
  /** Its state, as a mark before its title (a machine's tab: working, asks, idle). */
  mark?: State;
  count?: number;
  /** It can be closed. */
  closes?: boolean;
  /** Something new or waiting on you is in it, and you haven't looked. */
  dot?: boolean;
  /** Its icon alone: the title is its name and tooltip (the board's ✦, the chief of staff). */
  bare?: boolean;
  /** A control right after it (the board's ⇄ lead / channel). */
  after?: ComponentChildren;
  /** A visitor, not one of the place's own (the agent panel's peeked agent): its title in italics. */
  peek?: boolean;
  /** F2 starts renaming it. */
  rename?: () => void;
  /** Its name, being edited in place: `done` hears the new one, or null for none kept. */
  renaming?: { value: string; done: (name: string | null) => void };
  /** More in the tab after its title: who is in it, its badges. */
  extra?: ComponentChildren;
  /** More on the tab itself: its data-*, a drag, a menu, a middle click, a tooltip. */
  attrs?: Omit<JSX.IntrinsicElements["button"], "class" | "role" | "type" | "ref"> & { [data: `data-${string}`]: unknown };
}

export function Tabs({
  label,
  tabs,
  current,
  onPick,
  onClose,
  barRef,
  bar,
  marker,
  manual,
}: {
  label: string;
  tabs: TabItem[];
  current: string;
  onPick: (id: string) => void;
  onClose?: (id: string) => void;
  /** The tab list's element (a place that scrolls it, or keeps the shown tab in sight). */
  barRef?: Ref<HTMLDivElement>;
  /** More on the tab list: its data-*, its scroll and wheel. */
  bar?: Omit<JSX.IntrinsicElements["div"], "class" | "role" | "ref"> & { [data: `data-${string}`]: unknown };
  /** Where a drop would land, as an index: a marker there. */
  marker?: number | null;
  /** The arrows move the keys along the tabs and Enter shows one (a machine's tabs), not show each as they reach it. */
  manual?: boolean;
}) {
  const key = (e: KeyboardEvent, i: number) => {
    const to = { ArrowRight: i + 1, ArrowLeft: i - 1, Home: 0, End: tabs.length - 1 }[e.key];
    if (to !== undefined) {
      e.preventDefault();
      const t = tabs[(to + tabs.length) % tabs.length]!;
      if (!manual) onPick(t.id);
      (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(`[data-tab="${t.id}"]`)?.focus();
    } else if (e.key === "Delete" && tabs[i]!.closes) onClose?.(tabs[i]!.id);
    else if (e.key === "F2" && tabs[i]!.rename) (e.preventDefault(), tabs[i]!.rename!());
  };
  return (
    <div ref={barRef} class="k-tabs" role="tablist" aria-label={label} data-kit="tabs" {...bar}>
      {tabs.map((t, i) => {
        const on = t.id === current;
        const cls = ["k-tab", t.peek && "is-peek", on && "is-current"].filter(Boolean).join(" ");
        return [
          marker === i ? <span key={`m${i}`} class="k-tab-marker" aria-hidden="true" /> : null,
          t.renaming ? (
            <span key={t.id} class={cls} data-tab={t.id}>
              <input
                class="k-tab-rename"
                aria-label={`Rename ${t.title}`}
                value={t.renaming.value}
                ref={(el) => {
                  if (el) requestAnimationFrame(() => el.isConnected && document.activeElement !== el && (el.focus(), el.select()));
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") (e.preventDefault(), t.renaming!.done((e.currentTarget as HTMLInputElement).value.trim()));
                  else if (e.key === "Escape") (e.preventDefault(), e.stopPropagation(), t.renaming!.done(null));
                }}
                onBlur={(e) => t.renaming!.done((e.currentTarget as HTMLInputElement).value.trim())}
              />
            </span>
          ) : (
            <button key={t.id} type="button" class={cls} role="tab" data-tab={t.id} aria-selected={on} aria-label={t.bare ? t.title : undefined} title={t.bare ? t.title : undefined} tabIndex={on ? 0 : -1} onClick={() => onPick(t.id)} onKeyDown={(e) => key(e, i)} {...t.attrs}>
              {t.icon}
              {t.mark && <Mark state={t.mark} />}
              {!t.bare && <span class="k-tab-title">{t.title}</span>}
              {t.count ? <span class="m-meta">{t.count}</span> : null}
              {t.dot && <span class="k-pane-dot" aria-label="New" data-tab-dot />}
              {t.extra}
            </button>
          ),
          t.after ? <span key={`${t.id}-after`} class="k-tab-after">{t.after}</span> : null,
          t.closes && onClose ? (
            <button key={`${t.id}-x`} type="button" class="k-tab k-tab-x" tabIndex={-1} aria-label={`Close ${t.title}`} title={`Close ${t.title}`} data-tab-x={t.id} onClick={() => onClose(t.id)}>
              <CloseIcon size="xs" />
            </button>
          ) : null,
        ];
      })}
      {marker !== undefined && marker !== null && marker >= tabs.length ? <span class="k-tab-marker" aria-hidden="true" /> : null}
    </div>
  );
}
