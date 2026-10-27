// Right-click menus: one open at a time, closed by Escape, a click
// elsewhere, or choosing an item.

import { Fragment } from "preact";
import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import { useModal } from "./modal";

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

interface OpenMenu {
  x: number;
  y: number;
  items: MenuItem[];
  /** The keys go into it: its first row has the focus once it's shown
   * (UI audit #171). */
  focus: boolean;
}

/** How long a row asking to be confirmed ignores clicks: the second click
 * of a double-click lands here (the same as the confirm dialog's). */
const CONFIRM_SETTLE_MS = 500;

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

/** `opts.from`: the button that opens it (a ▾, a …): it gets
 * `aria-expanded`, the first item gets the keys, and the focus goes back
 * to it when the menu shuts. */
export function openMenu(e: { clientX: number; clientY: number; preventDefault(): void }, items: MenuItem[], opts: { from?: HTMLElement | null } | HTMLElement = {}) {
  e.preventDefault();
  if (opts instanceof HTMLElement) opts = { from: opts };
  if (!current) opener = opts.from ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
  expanded(opts.from ?? null);
  current = { x: e.clientX, y: e.clientY, items, focus: true };
  changed();
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
    current = null;
    changed();
  }
  opener = null;
}

export function MenuLayer() {
  const [, setTick] = useState(0);
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const [me] = useState(() => ({}));
  /** The submenu open, by its row's index: one at a time. */
  const [sub, setSub] = useState<number | null>(null);
  /** A key opened a submenu: its first item gets the keys once drawn. */
  const intoSub = useRef(false);
  /** When a click opened the submenu: its rows ignore clicks for a moment,
   * as it scrolls into view and rows slide under the pointer, so a
   * double-click on its row doesn't choose one of them. */
  const subOpenedAt = useRef(0);
  /** The row asking to be confirmed: its label, and when it asked. */
  const [armed, setArmed] = useState<{ label: string; at: number } | null>(null);

  useEffect(() => {
    const fn = () => setTick((t) => t + 1);
    listeners.add(fn);
    layers.push(me);
    changed();
    const key = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || !current) return;
      tookEscape.add(e);
      closeAndReturn();
    };
    const down = (e: PointerEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) closeMenu();
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
    setPos({
      left: Math.max(8, Math.min(current.x, window.innerWidth - r.width - 8)),
      top: Math.max(8, Math.min(current.y, window.innerHeight - r.height - 8)),
    });
  }, [current, top]);
  useEffect(() => {
    setSub(null);
    setArmed(null);
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
  // The arrows, Home and End move between the items.
  const onKey = (e: KeyboardEvent) => {
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

  const renderItem = (item: MenuItem, i: number, inSub = false) =>
    item === "separator" ? (
      <div key={i} class="menu-sep" />
    ) : "header" in item ? (
      <Fragment key={i}>
        <div class={item.name ? "menu-header named" : "menu-header"}>{item.header}</div>
        {item.note && <div class="menu-note">{item.note}</div>}
      </Fragment>
    ) : "submenu" in item ? null : (
      <button
        key={i}
        role={item.checked === undefined ? "menuitem" : "menuitemradio"}
        aria-checked={item.checked}
        aria-current={item.current || undefined}
        aria-keyshortcuts={item.shortcut?.replace("Ctrl", "Control")}
        class={`menu-item${item.danger || (item.confirm && armed?.label === item.label) ? " danger" : ""}${item.faded ? " faded" : ""}`}
        disabled={item.disabled}
        title={item.hint}
        onClick={() => {
          if (inSub && Date.now() - subOpenedAt.current < CONFIRM_SETTLE_MS) return;
          if (item.confirm) {
            if (!armed || armed.label !== item.label) return setArmed({ label: item.label, at: Date.now() });
            if (Date.now() - armed.at < CONFIRM_SETTLE_MS) return;
          }
          // Shut for good before the item runs: a dialog it opens finds
          // no menu left over, and the focus where the menu was opened.
          closeAndReturn();
          item.run();
        }}
      >
        {(item.checked ?? item.current) !== undefined && (
          <span class="menu-check" aria-hidden="true">
            {item.checked || item.current ? "●" : ""}
          </span>
        )}
        {item.confirm && armed?.label === item.label ? item.confirm : item.label}
        {item.detail && (
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

  if (!current || !top) return null;
  return (
    <div
      ref={ref}
      class="menu"
      role="menu"
      style={{
        left: `${pos?.left ?? current.x}px`,
        top: `${pos?.top ?? current.y}px`,
        visibility: pos ? "visible" : "hidden",
        // Where it was placed it stays: a submenu opening under the pointer
        // makes it scroll, never move (and a gutter of 8px below it).
        boxSizing: "border-box",
        maxHeight: `${window.innerHeight - (pos?.top ?? 8) - 8}px`,
      }}
      onContextMenu={(e) => e.preventDefault()}
      onKeyDown={onKey}
    >
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
              {item.label}
              <span class="menu-chevron" aria-hidden="true">
                {sub === i ? "▾" : "▸"}
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
  if (!prompt) return null;
  const p = prompt;
  const finish = finishPrompt;
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
          finish(input.current?.value ?? "");
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
            onInput={(e) => (p.value = (e.target as HTMLInputElement).value)}
            placeholder={p.placeholder}
          />
        </label>
        )}
        {p.choices && (
          <div class="prompt-choices" role="group" aria-label="Choices">
            {p.choices.map((c) => (
              <button type="button" class={c.strong ? "strong" : undefined} title={c.title} onClick={() => finish(c.value)}>
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
