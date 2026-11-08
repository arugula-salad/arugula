// Right-click menus: one open at a time, closed by Escape, a click
// elsewhere, or choosing an item. A `Popover` is the same box for content
// that isn't a list of items (chat's agents and emoji): it hangs from its
// button as a menu does, is a sheet on a phone, closes the same ways, and
// keeps the focus inside until it does.

import { Fragment, type ComponentChildren } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { useModal } from "./modal";
import { ChevronIcon } from "./icons";

export type MenuItem =
  | {
      label: string;
      run: () => void;
      danger?: boolean;
      disabled?: boolean;
      /** Why it is disabled, or what it does: the tooltip. */
      hint?: string;
      checked?: boolean;
      /** Its chord, shown beside it here and in the command palette. */
      shortcut?: string;
      /** What follows its name, dim, cut short when the menu is narrow: a
       * machine's panes and state (UI audit #175). */
      detail?: string;
      /** Shown dim (an offline machine), though it can still be chosen. */
      faded?: boolean;
      /** In a list with one current (the machine shown): its ●, the same
       * mark as a checked item's, with aria-current. */
      current?: boolean;
      /** The action it is, when two menus offer the same one (the pane's
       * and the tab's Go to directory…): the command palette lists it
       * once (UI audit #195). */
      id?: string;
      /** For what can't be undone: the first click keeps the menu open and
       * shows this in the row's place ("Forget history? Click again to
       * confirm"); the second runs it. A second click straight after the
       * first (a double-click) is ignored (#328). */
      confirm?: string;
      /** A 16px mark for the row's lead column (an `Svg` of the rail's). A
       * menu gets the column when any row has an icon or a mark, and then
       * every row keeps it, so the labels line up. */
      icon?: ComponentChildren;
      /** With `checked`: one of several that can be ticked together. Its mark
       * is a ✓, it is a `menuitemcheckbox`, and choosing it ticks it in place
       * and leaves the menu open. (A plain `checked` is a ● and one of.) */
      multi?: boolean;
      /** Shown in the row's place for a beat after it runs ("Copied"), then
       * the menu shuts. */
      done?: string;
      /** Attributes for the row, such as a `data-` hook for the specs. */
      attrs?: Attrs;
    }
  /** `name`: the header holds a name (a channel's, a login), shown as
   * written rather than in capitals (UI audit #169). `note`: a line under
   * it saying what the items do (UI audit #163). */
  | { header: string; name?: boolean; note?: string }
  /** A row that opens its items under it, indented, rather than a flyout:
   * a long menu stays short, and a phone has no hover. The command palette
   * lists them as if they were in the menu. */
  | { label: string; submenu: MenuItem[]; id?: string }
  | "separator";

type Attrs = Record<string, string | boolean>;

/** What a menu takes besides its items. */
export interface MenuOpts {
  /** The button that opens it (a ▾, a …): it gets `aria-expanded`, the first
   * item gets the keys, and the focus goes back to it when the menu shuts. */
  from?: HTMLElement | null;
  /** The menu's right edge is at x, not its left (a ⋯ at a row's end). */
  end?: boolean;
  /** Its accessible name. */
  label?: string;
  /** Attributes for the menu itself, such as a `data-` hook for the specs. */
  attrs?: Attrs;
  /** Drawn above the items (chat's emoji row). */
  head?: ComponentChildren;
  /** It shut, by whatever means. */
  onClose?: () => void;
}

/** A phone's width: a menu is a sheet along the bottom there, over the same
 * `--backdrop` a dialog has. */
const SHEET = "(max-width: 600px)";

interface OpenMenu extends MenuOpts {
  x: number;
  y: number;
  items: MenuItem[];
  /** The keys go into it: its first row has the focus once it's shown
   * (UI audit #171). */
  focus: boolean;
  /** A popover's content, in place of the items. */
  content?: ComponentChildren;
}

/** How long a row asking to be confirmed ignores clicks: the second click
 * of a double-click lands here (the same as the confirm dialog's). */
const CONFIRM_SETTLE_MS = 500;
/** How long a row that says it ran ("Copied") is shown before the menu shuts. */
const DONE_MS = 700;

let current: OpenMenu | null = null;
/** What had the focus when the menu opened: an item gives it back before it
 * runs, so a dialog the item opens returns the focus there when it shuts
 * (UI audit #150), and Escape gives it back too. */
let opener: HTMLElement | null = null;
/** The menu button it was opened from, which says it's expanded while it
 * is (UI audit #171). */
let from: HTMLElement | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
/** The layers mounted, latest last: only it draws the menu (the swarm's,
 * over the tab view's), else each would close the other's on a click. */
const layers: object[] = [];

/** The menu a press on its own button just shut: the click that follows
 * would open it again, so it stays shut (the button is a toggle). */
let pressShut: { el: HTMLElement; at: number } | null = null;
const PRESS_SHUT_MS = 600;

/** A menu's lines only between rows: a menu built from parts that can each
 * be empty (the pane's, when a part has nothing for this pane) would
 * otherwise draw two lines together, or one at an end. Submenus too. */
export function tidy(items: MenuItem[]): MenuItem[] {
  const out: MenuItem[] = [];
  for (const it of items) {
    if (it === "separator" && (out.length === 0 || out[out.length - 1] === "separator")) continue;
    out.push(typeof it === "object" && "submenu" in it ? { ...it, submenu: tidy(it.submenu) } : it);
  }
  while (out[out.length - 1] === "separator") out.pop();
  return out;
}

/** `opts.from`: the button that opens it (a ▾, a …): it gets
 * `aria-expanded`, the first item gets the keys, and the focus goes back
 * to it when the menu shuts. False when it stayed shut: the press on the
 * button had just shut it. */
export function openMenu(e: { clientX: number; clientY: number; preventDefault(): void }, items: MenuItem[], opts: MenuOpts | HTMLElement = {}): boolean {
  e.preventDefault();
  if (opts instanceof HTMLElement) opts = { from: opts };
  if (opts.from && pressShut?.el === opts.from && Date.now() - pressShut.at < PRESS_SHUT_MS) {
    pressShut = null;
    return false;
  }
  pressShut = null;
  if (current) current.onClose?.();
  else opener = opts.from ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
  expanded(opts.from ?? null);
  current = { ...opts, x: e.clientX, y: e.clientY, items: tidy(items), focus: true };
  changed();
  return true;
}

/** Where a menu hangs from a button: under it, its left edge at the
 * button's (or, with `end`, its right edge at the button's: pass `end` to
 * the menu too). */
export function menuBelow(el: HTMLElement, end = false) {
  const r = el.getBoundingClientRect();
  return { clientX: end ? r.right : r.left, clientY: r.bottom + 4, preventDefault() {} };
}

/** Whether a press on `el` just shut the menu or popover that hung from it:
 * the click that follows is the same tap, so a button that toggles one
 * leaves it shut. Asking consumes it. */
export function pressedShut(el: HTMLElement | null): boolean {
  const was = !!el && pressShut?.el === el && Date.now() - pressShut.at < PRESS_SHUT_MS;
  if (was) pressShut = null;
  return was;
}

function expanded(el: HTMLElement | null) {
  if (from && from !== el) from.setAttribute("aria-expanded", "false");
  from = el;
  el?.setAttribute("aria-expanded", "true");
}

/** Whether the menu open now hangs from `el`, for a menu button's
 * aria-expanded in its own markup (UI audit #174). */
export function useMenuOpenFrom(el: { current: HTMLElement | null }): boolean {
  const [, setTick] = useState(0);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    return () => void listeners.delete(fn);
  }, []);
  return !!current && !!from && from === el.current;
}

/** The same, by the name the platform bar knows it (UI audit #185). */
export const useMenuFrom = useMenuOpenFrom;

/** Close it, and put the focus back where it was when it opened. */
function closeAndReturn() {
  const to = opener;
  closeMenu();
  if (to?.isConnected && to !== document.body) to.focus({ preventScroll: true });
}

/** Escapes a menu took (closing itself): a view under it lets them be. */
const tookEscape = new WeakSet<Event>();
export const escapeTaken = (e: Event) => tookEscape.has(e);

export function closeMenu() {
  expanded(null);
  if (current) {
    const was = current;
    current = null;
    changed();
    was.onClose?.();
  }
  opener = null;
}

/** A menu drawn where it is used, inside the element that has it, rather than
 * at the page's one layer: mounted when `show` should open it (it calls
 * `openMenu`, which says whether it did), unmounted by `close`, which the
 * menu's `onClose` calls too. The menu belongs to the thing it is for, so a
 * spec (or a style) can reach it from there. */
export function MenuHost({ show, close }: { show: () => boolean; close: () => void }) {
  useEffect(() => {
    if (!show()) close();
  }, []);
  return <MenuLayer />;
}

/** Arbitrary content in the menu's box, hanging from the button `from`
 * (below it, or above when there's no room; its right edge at the button's
 * with `end`) and a sheet along the bottom on a phone. Mounted while it's
 * open: `onClose` is told when the menu's own ways shut it (Escape, a click
 * elsewhere, another menu), and the caller drops it. Escape puts the focus
 * back on `from`; a caller that closes it on a choice puts it where it
 * wants. What inside is marked `data-autofocus` has the focus once it's
 * shown, else the popover itself; Tab stays inside it. It is drawn where it
 * is mounted, so a spec can reach it from its owner. A press on `from`
 * shuts it: check `pressedShut(from)` before opening it again. */
export function Popover({
  from,
  end,
  label,
  attrs,
  onClose,
  children,
}: {
  from: { current: HTMLElement | null };
  end?: boolean;
  /** Its accessible name. */
  label: string;
  /** Attributes for the popover itself, such as a `data-` hook for the specs. */
  attrs?: Attrs;
  onClose: () => void;
  children: ComponentChildren;
}) {
  const me = useRef<OpenMenu | null>(null);
  const [hold] = useState(() => ({}));
  const told = useRef(onClose);
  told.current = onClose;
  useLayoutEffect(() => {
    const el = from.current;
    if (!el) return told.current();
    const at = menuBelow(el, end);
    let leaving = false;
    const mine: OpenMenu = {
      x: at.clientX,
      y: at.clientY,
      items: [],
      focus: false,
      content: children,
      from: el,
      end,
      label,
      attrs,
      onClose: () => !leaving && told.current(),
    };
    pressShut = null;
    if (current) current.onClose?.();
    else opener = el;
    // Its own layer (below) is the one that draws it: held here so that no
    // other layer, such as the page's, draws it first and drops it again.
    layers.push(hold);
    expanded(el);
    current = mine;
    me.current = mine;
    changed();
    return () => {
      leaving = true;
      const held = layers.indexOf(hold);
      if (held >= 0) layers.splice(held, 1);
      if (current === mine) closeMenu();
      else changed();
    };
  }, []);
  // What it shows follows the caller's render: a choice made in it uses
  // the caller's state as it is now.
  useLayoutEffect(() => {
    if (me.current && current === me.current && me.current.content !== children) {
      me.current.content = children;
      changed();
    }
  });
  // Drawn here, inside the element that has it, as `MenuHost` does.
  return <MenuLayer />;
}

/** What can take the Tab key in a popover. */
const TABBABLE = 'a[href], button:not(:disabled), input:not(:disabled), select:not(:disabled), textarea:not(:disabled), [tabindex]:not([tabindex="-1"])';

export function MenuLayer() {
  const [, setTick] = useState(0);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const [me] = useState(() => ({}));
  /** The submenu open, by its row's index: one at a time. */
  const [sub, setSub] = useState<number | null>(null);
  /** A key opened a submenu: its first item gets the keys once drawn. */
  const intoSub = useRef(false);
  /** When a click opened the submenu: for a moment, its rows ignore the
   * second click of a double-click, as it scrolls into view and rows slide
   * under the pointer, so a double-click on its row doesn't choose one of
   * them. A single click on a row goes through at once. */
  const subOpenedAt = useRef(0);
  /** The row asking to be confirmed: its label, and when it asked. */
  const [armed, setArmed] = useState<{ label: string; at: number } | null>(null);
  /** Several-at-once rows ticked or cleared since it opened, by row: the
   * menu stays open, so it draws their new state itself. */
  const [ticks, setTicks] = useState<Record<string, boolean>>({});
  /** The row that ran and says so ("Copied") for a beat before it shuts. */
  const [done, setDone] = useState<string | null>(null);

  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    layers.push(me);
    changed();
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !current) return;
      tookEscape.add(e);
      // A popover's Escape is chat's: marked handled, so the page and the
      // desktop app's window leave it be (#380).
      if (current.content !== undefined) e.preventDefault();
      closeAndReturn();
    };
    const down = (e: PointerEvent) => {
      const t = e.target as Element;
      // The sheet's backdrop closes it on its click, so the tap that shuts it
      // doesn't fall through to what is under it.
      if (!ref.current || ref.current.contains(t) || t.closest?.("[data-menu-backdrop]")) return;
      if (from?.contains(t)) pressShut = { el: from, at: Date.now() };
      closeMenu();
    };
    window.addEventListener("keydown", key, true);
    window.addEventListener("pointerdown", down, true);
    window.addEventListener("blur", closeMenu);
    return () => {
      listeners.delete(fn);
      layers.splice(layers.indexOf(me), 1);
      changed();
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("pointerdown", down, true);
      window.removeEventListener("blur", closeMenu);
    };
  }, []);

  const top = layers.at(-1) === me;
  // Keep the menu on screen, a gutter from its edges (UI audit #171).
  const focusIn = useRef(false);
  useLayoutEffect(() => {
    if (!current || !ref.current) return setPos(null);
    // Measured at the window's left, where nothing squeezes it: near the
    // right edge it wrapped ("Command palette… Ctrl+Shift+P") and then sat
    // flush with the edge, wider than measured (UI audit #174).
    const was = ref.current.style.left;
    const wasMax = ref.current.style.maxHeight;
    ref.current.style.left = "0px";
    ref.current.style.maxHeight = `${window.innerHeight - 16}px`;
    const r = ref.current.getBoundingClientRect();
    ref.current.style.left = was;
    ref.current.style.maxHeight = wasMax;
    focusIn.current = current.focus;
    if (matchMedia(SHEET).matches) return setPos({ left: 0, top: 0 });
    const x = current.end ? current.x - r.width : current.x;
    let y = current.y;
    // A popover with no room below its button opens above it, when that has
    // more (the composer's emoji, at the foot of the page).
    if (current.content !== undefined && current.from && y + r.height > window.innerHeight - 8) {
      const f = current.from.getBoundingClientRect();
      if (f.top - 4 >= r.height + 8 || f.top > window.innerHeight - f.bottom) y = f.top - 4 - r.height;
    }
    setPos({
      left: Math.max(8, Math.min(x, window.innerWidth - r.width - 8)),
      top: Math.max(8, Math.min(y, window.innerHeight - r.height - 8)),
    });
  }, [current, top]);
  useEffect(() => {
    setSub(null);
    setArmed(null);
    setTicks({});
    setDone(null);
  }, [current]);
  // An opened submenu is scrolled into view, by the least that shows it,
  // and never past its own row: that stays shown.
  useLayoutEffect(() => {
    const menu = ref.current;
    const rows = menu?.querySelector<HTMLElement>(".menu-sub");
    if (sub === null || !menu || !rows) return;
    const m = menu.getBoundingClientRect();
    const pad = parseFloat(getComputedStyle(menu).paddingBottom) || 0;
    const need = rows.getBoundingClientRect().bottom - (m.bottom - pad);
    const row = rows.previousElementSibling?.getBoundingClientRect().top ?? m.top;
    if (need > 0) menu.scrollTop += Math.max(0, Math.min(need, row - m.top));
  }, [sub]);
  useEffect(() => {
    if (sub === null || !intoSub.current) return;
    intoSub.current = false;
    ref.current?.querySelector<HTMLElement>(".menu-sub .menu-item:not(:disabled)")?.focus({ preventScroll: true });
  }, [sub]);
  // From a menu button, the first item has the keys once it's shown.
  useEffect(() => {
    if (!pos || !focusIn.current) return;
    focusIn.current = false;
    ref.current?.querySelector<HTMLElement>(".menu-item:not(:disabled)")?.focus({ preventScroll: true });
  }, [pos]);
  // A popover takes the focus once it's shown (it is hidden until placed, and
  // a hidden element can't have it): what is marked `data-autofocus`, else
  // the popover itself, so the Tab key starts inside it.
  const shownFor = useRef<OpenMenu | null>(null);
  useEffect(() => {
    if (!pos || !current || current.content === undefined || shownFor.current === current) return;
    const el = ref.current;
    if (!el || getComputedStyle(el).visibility !== "visible") return;
    shownFor.current = current;
    if (el.contains(document.activeElement)) return;
    (el.querySelector<HTMLElement>("[data-autofocus]") ?? el).focus({ preventScroll: true });
  }, [pos, current]);
  // The arrows, Home and End move between the items.
  const onKey = (e: KeyboardEvent) => {
    // A popover keeps the Tab key: from its last control to its first, and
    // back, rather than out to the page behind it.
    if (e.key === "Tab" && current?.content !== undefined) {
      const tabs = [...(ref.current?.querySelectorAll<HTMLElement>(TABBABLE) ?? [])];
      const at = document.activeElement;
      const wrap = !tabs.length || (e.shiftKey ? at === tabs[0] || at === ref.current : at === tabs[tabs.length - 1]);
      if (wrap) {
        e.preventDefault();
        (e.shiftKey ? tabs[tabs.length - 1] : tabs[0] ?? ref.current)?.focus({ preventScroll: true });
      }
      return;
    }
    const items = [...(ref.current?.querySelectorAll<HTMLElement>(".menu-item:not(:disabled)") ?? [])];
    if (!items.length) return;
    const i = items.indexOf(document.activeElement as HTMLElement);
    const to =
      e.key === "ArrowDown" ? (i + 1) % items.length
      : e.key === "ArrowUp" ? (i <= 0 ? items.length - 1 : i - 1)
      : e.key === "Home" ? 0
      : e.key === "End" ? items.length - 1
      : null;
    const at = document.activeElement as HTMLElement | null;
    // → opens a submenu and goes in; ← shuts it and goes back to its row.
    if (e.key === "ArrowRight" && at?.getAttribute("aria-haspopup")) {
      e.preventDefault();
      e.stopPropagation();
      const n = Number(at.dataset.sub);
      if (sub === n) at.nextElementSibling?.querySelector<HTMLElement>(".menu-item:not(:disabled)")?.focus();
      else {
        intoSub.current = true;
        setSub(n);
      }
      return;
    }
    if (e.key === "ArrowLeft" && at?.closest(".menu-sub")) {
      e.preventDefault();
      e.stopPropagation();
      const row = at.closest(".menu-sub")?.previousElementSibling as HTMLElement | null;
      setSub(null);
      row?.focus();
      return;
    }
    if (to === null) return;
    e.preventDefault();
    e.stopPropagation();
    items[to].focus();
  };

  /** Whether any row has an icon or a mark: then every row has the column. */
  const lead =
    !!current &&
    current.items.some((it) =>
      typeof it !== "object" || "header" in it
        ? false
        : "submenu" in it
          ? it.submenu.some((x) => typeof x === "object" && "label" in x && "run" in x && (x.icon || x.checked !== undefined || x.current !== undefined))
          : !!it.icon || it.checked !== undefined || it.current !== undefined,
    );

  const renderItem = (item: MenuItem, i: number, inSub = false) => {
    if (item === "separator") return <div key={i} class="menu-sep" />;
    if ("header" in item)
      return (
        <Fragment key={i}>
          <div class={item.name ? "menu-header named" : "menu-header"}>{item.header}</div>
          {item.note && <div class="menu-note">{item.note}</div>}
        </Fragment>
      );
    if ("submenu" in item) return null;
    const key = `${inSub ? `${sub}.` : ""}${i}`;
    const on = item.multi ? (ticks[key] ?? !!item.checked) : item.checked;
    const mark = item.multi ? (on ? "✓" : "") : item.checked || item.current ? "●" : "";
    const marked = item.checked !== undefined || item.current !== undefined;
    const saying = done === key;
    return (
      <button
        key={i}
        role={item.multi ? "menuitemcheckbox" : item.checked === undefined ? "menuitem" : "menuitemradio"}
        aria-checked={on}
        aria-current={item.current || undefined}
        aria-keyshortcuts={item.shortcut?.replace("Ctrl", "Control")}
        class={`menu-item${item.danger || (item.confirm && armed?.label === item.label) ? " danger" : ""}${item.faded ? " faded" : ""}`}
        disabled={item.disabled || saying}
        title={item.hint}
        {...item.attrs}
        onClick={(e) => {
          if (inSub && e.detail > 1 && Date.now() - subOpenedAt.current < CONFIRM_SETTLE_MS) return;
          if (item.confirm) {
            if (!armed || armed.label !== item.label) return setArmed({ label: item.label, at: Date.now() });
            if (Date.now() - armed.at < CONFIRM_SETTLE_MS) return;
          }
          if (item.multi) {
            setTicks({ ...ticks, [key]: !on });
            item.run();
            return;
          }
          if (item.done) {
            const shown = current;
            setDone(key);
            item.run();
            setTimeout(() => current === shown && closeAndReturn(), DONE_MS);
            return;
          }
          // Shut for good before the item runs: a dialog it opens finds
          // no menu left over, and the focus where the menu was opened.
          closeAndReturn();
          item.run();
        }}
      >
        {lead && (
          <span class={marked ? "menu-lead menu-check" : "menu-lead"} aria-hidden="true">
            {marked ? mark : item.icon}
          </span>
        )}
        {saying ? item.done : item.confirm && armed?.label === item.label ? item.confirm : item.label}
        {item.detail && !saying && (
          <>
            {" "}
            <span class="menu-detail">{item.detail}</span>
          </>
        )}
        {item.shortcut && (
          <kbd class="menu-key" aria-hidden="true">
            {item.shortcut}
          </kbd>
        )}
      </button>
    );
  };

  if (!current || !top) return null;
  const sheet = matchMedia(SHEET).matches;
  const popover = current.content !== undefined;
  return (
    <>
      {sheet && <div class="menu-backdrop" data-menu-backdrop onClick={closeAndReturn} />}
      <div
        ref={ref}
        class={`menu${sheet ? " sheet" : ""}${popover ? " popover" : ""}`}
        role={popover ? "dialog" : "menu"}
        tabIndex={popover ? -1 : undefined}
        aria-label={current.label}
        {...current.attrs}
        style={
          sheet
            ? { visibility: pos ? "visible" : "hidden" }
            : {
                left: `${pos?.left ?? current.x}px`,
                top: `${pos?.top ?? current.y}px`,
                visibility: pos ? "visible" : "hidden",
                // Where it was placed it stays: a submenu opening under the pointer
                // makes it scroll, never move (and a gutter of 8px below it).
                boxSizing: "border-box",
                maxHeight: `${window.innerHeight - (pos?.top ?? 8) - 8}px`,
              }
        }
        onContextMenu={(e) => e.preventDefault()}
        onKeyDown={onKey}
      >
        {current.head}
        {current.content}
        {current.items.map((item, i) =>
          typeof item === "object" && "submenu" in item ? (
            <Fragment key={i}>
              <button
                role="menuitem"
                aria-haspopup="menu"
                aria-expanded={sub === i}
                data-sub={i}
                class="menu-item menu-parent"
                onClick={(e) => {
                  subOpenedAt.current = sub !== i && e.detail > 0 ? Date.now() : 0;
                  setSub(sub === i ? null : i);
                }}
              >
                {lead && <span class="menu-lead" aria-hidden="true" />}
                {item.label}
                <span class="menu-chevron" aria-hidden="true">
                  <ChevronIcon open={sub === i} />
                </span>
              </button>
              {sub === i && (
                <div class="menu-sub" role="group" aria-label={item.label}>
                  {item.submenu.map((it, j) => renderItem(it, j, true))}
                </div>
              )}
            </Fragment>
          ) : (
            renderItem(item, i)
          ),
        )}
      </div>
    </>
  );
}

// ---------------------------------------------------------------- prompt

interface OpenPrompt {
  title: string;
  value: string;
  placeholder?: string;
  /** No input: a note to read, with a link to more (#171). */
  notice?: { text: string; link: { label: string; href: string } };
  /** Answers to pick with a tap, once they're known (#150). */
  choices?: PromptChoice[];
  /** The choice the arrow keys are on, among those the input leaves. */
  active?: number;
  done: (value: string | null) => void;
}

/** One answer offered under the input: choosing it answers with `value`. */
export interface PromptChoice {
  value: string;
  label: string;
  /** Said beside the label, dimmer (the program on a port). */
  hint?: string;
  title?: string;
  /** Shown first and marked (the pane's own port). */
  strong?: boolean;
}

let prompt: OpenPrompt | null = null;
const promptListeners = new Set<() => void>();

/** Ask for one line of text (no browser dialogs: they block everything).
 * `choices`, when they arrive while it's open, show under the input. */
export function askText(
  title: string,
  value: string,
  placeholder?: string,
  choices?: Promise<PromptChoice[]>,
): Promise<string | null> {
  return new Promise((resolve) => {
    const p: OpenPrompt = { title, value, placeholder, done: resolve };
    prompt = p;
    promptListeners.forEach((fn) => fn());
    void choices
      ?.then((list) => {
        if (prompt !== p || list.length === 0) return;
        p.choices = list;
        promptListeners.forEach((fn) => fn());
      })
      .catch(() => {});
  });
}

/** Say why something isn't there yet and where to read how to turn it
 * on, instead of an error (#171). */
export function tellSetup(title: string, text: string, link: { label: string; href: string }): Promise<void> {
  return new Promise((resolve) => {
    prompt = { title, value: "", notice: { text, link }, done: () => resolve() };
    promptListeners.forEach((fn) => fn());
  });
}

/** The choices the input leaves: those whose label or hint has what was
 * typed, any case. */
function shownChoices(p: OpenPrompt): PromptChoice[] {
  const q = p.value.trim().toLowerCase();
  return (p.choices ?? []).filter((c) => !q || `${c.label} ${c.hint ?? ""}`.toLowerCase().includes(q));
}

/** Answer the open prompt (null: cancelled). */
function finishPrompt(v: string | null) {
  const p = prompt;
  if (!p) return;
  prompt = null;
  promptListeners.forEach((fn) => fn());
  p.done(v);
}

export function PromptLayer() {
  const [, setTick] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const form = useRef<HTMLFormElement>(null);
  // UI audit #150: a dialog like the others (Escape, the focus kept in).
  useModal(form, () => finishPrompt(null), !!prompt);
  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    promptListeners.add(fn);
    return () => {
      promptListeners.delete(fn);
    };
  }, []);
  useLayoutEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [prompt]);
  useLayoutEffect(() => {
    form.current?.querySelector(".prompt-choices .active")?.scrollIntoView({ block: "nearest" });
  });
  if (!prompt) return null;
  const p = prompt;
  const finish = finishPrompt;
  const shown = shownChoices(p);
  const active = p.active !== undefined && p.active < shown.length ? p.active : undefined;
  return (
    <div
      class="prompt-backdrop"
      role="dialog"
      aria-label={p.title}
      onPointerDown={(e) => e.target === e.currentTarget && finish(null)}
    >
      <form
        ref={form}
        class="prompt"
        aria-label={p.title}
        onSubmit={(e) => {
          e.preventDefault();
          finish(active === undefined ? (input.current?.value ?? "") : shown[active].value);
        }}
      >
        {p.notice ? (
          <div class="prompt-notice" data-setup-notice>
            <b>{p.title}</b>
            <p>{p.notice.text}</p>
            <p>
              <a href={p.notice.link.href} target="_blank" rel="noopener noreferrer">
                {p.notice.link.label}
              </a>
            </p>
          </div>
        ) : (
        <label>
          {p.title}
          <input
            ref={input}
            value={p.value}
            // Kept as typed: the page re-renders as panes change.
            onInput={(e) => {
              p.value = (e.target as HTMLInputElement).value;
              p.active = undefined;
              setTick((t) => t + 1);
            }}
            onKeyDown={(e) => {
              if ((e.key !== "ArrowDown" && e.key !== "ArrowUp") || shown.length === 0) return;
              e.preventDefault();
              const step = e.key === "ArrowDown" ? 1 : -1;
              p.active = active === undefined ? (step > 0 ? 0 : shown.length - 1) : (active + step + shown.length) % shown.length;
              setTick((t) => t + 1);
            }}
            placeholder={p.placeholder}
          />
        </label>
        )}
        {p.choices && (
          <div class="prompt-choices" role="group" aria-label="Choices">
            {shown.map((c, i) => (
              <button type="button" class={[c.strong && "strong", i === active && "active"].filter(Boolean).join(" ") || undefined} title={c.title} onClick={() => finish(c.value)}>
                {c.label}
                {c.hint && <span class="dim"> {c.hint}</span>}
              </button>
            ))}
          </div>
        )}
        <div class="prompt-buttons">
          {!p.notice && (
            <button class="secondary" type="button" onClick={() => finish(null)}>
              Cancel
            </button>
          )}
          <button type="submit" class="primary">
            OK
          </button>
        </div>
      </form>
    </div>
  );
}
