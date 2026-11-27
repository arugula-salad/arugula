// Tabs (kit §4.1, 11, the tabs half; the panes are ./panes/): an icon, a
// title, a count, a dot (new, or waiting on you) and a close. The arrow keys, Home and End move
// between them; Delete closes the focused one.

import type { ComponentChildren } from "preact";
import { CloseIcon } from "../ui/icons";
import "./kit.css";

export interface TabItem {
  id: string;
  title: string;
  icon?: ComponentChildren;
  count?: number;
  /** It can be closed. */
  closes?: boolean;
  /** Something new or waiting on you is in it, and you haven't looked. */
  dot?: boolean;
  /** A visitor, not one of the place's own (the agent panel's peeked agent): its title in italics. */
  peek?: boolean;
}

export function Tabs({ label, tabs, current, onPick, onClose }: { label: string; tabs: TabItem[]; current: string; onPick: (id: string) => void; onClose?: (id: string) => void }) {
  const key = (e: KeyboardEvent, i: number) => {
    const to = { ArrowRight: i + 1, ArrowLeft: i - 1, Home: 0, End: tabs.length - 1 }[e.key];
    if (to !== undefined) {
      e.preventDefault();
      const t = tabs[(to + tabs.length) % tabs.length]!;
      onPick(t.id);
      (e.currentTarget as HTMLElement).parentElement?.querySelector<HTMLElement>(`[data-tab="${t.id}"]`)?.focus();
    } else if (e.key === "Delete" && tabs[i]!.closes) onClose?.(tabs[i]!.id);
  };
  return (
    <div class="k-tabs" role="tablist" aria-label={label} data-kit="tabs">
      {tabs.map((t, i) => {
        const on = t.id === current;
        return [
          <button key={t.id} type="button" class={t.peek ? "k-tab is-peek" : "k-tab"} role="tab" data-tab={t.id} aria-selected={on} tabIndex={on ? 0 : -1} onClick={() => onPick(t.id)} onKeyDown={(e) => key(e, i)}>
            {t.icon}
            <span class="k-tab-title">{t.title}</span>
            {t.count ? <span class="m-meta">{t.count}</span> : null}
            {t.dot && <span class="k-pane-dot" aria-label="New" data-tab-dot />}
          </button>,
          t.closes && onClose ? (
            <button key={`${t.id}-x`} type="button" class="k-tab k-tab-x" tabIndex={-1} aria-label={`Close ${t.title}`} title={`Close ${t.title}`} data-tab-x={t.id} onClick={() => onClose(t.id)}>
              <CloseIcon size="xs" />
            </button>
          ) : null,
        ];
      })}
    </div>
  );
}
