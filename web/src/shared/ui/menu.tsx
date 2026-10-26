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
    }
  /** `note`: a line under the header saying what the items do (UI audit
   * #163), in sentence case where the header is a word or two. */
  | { header: string; note?: string }
  | "separator";

interface OpenMenu {
  x: number;
  y: number;
  items: MenuItem[];
}

let current: OpenMenu | null = null;
/** What had the focus when the menu opened: an item gives it back before it
 * runs, so a dialog the item opens returns the focus there when it shuts
 * (UI audit #150), and Escape gives it back too. */
let opener: HTMLElement | null = null;
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((fn) => fn());
/** The layers mounted, latest last: only it draws the menu (the swarm's,
 * over the tab view's), else each would close the other's on a click. */
const layers: object[] = [];

export function openMenu(e: { clientX: number; clientY: number; preventDefault(): void }, items: MenuItem[]) {
  e.preventDefault();
  if (!current) opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
  current = { x: e.clientX, y: e.clientY, items };
  changed();
}

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
  // Keep the menu on screen.
  useLayoutEffect(() => {
    if (!current || !ref.current) return setPos(null);
    const r = ref.current.getBoundingClientRect();
    setPos({
      left: Math.max(4, Math.min(current.x, window.innerWidth - r.width - 4)),
      top: Math.max(4, Math.min(current.y, window.innerHeight - r.height - 4)),
    });
  }, [current, top]);

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
      }}
      onContextMenu={(e) => e.preventDefault()}
    >
      {current.items.map((item, i) =>
        item === "separator" ? (
          <div key={i} class="menu-sep" />
        ) : "header" in item ? (
          <Fragment key={i}>
            <div class="menu-header">{item.header}</div>
            {item.note && <div class="menu-note">{item.note}</div>}
          </Fragment>
        ) : (
          <button
            key={i}
            role={item.checked === undefined ? "menuitem" : "menuitemradio"}
            aria-checked={item.checked}
            aria-keyshortcuts={item.shortcut?.replace("Ctrl", "Control")}
            class={item.danger ? "menu-item danger" : "menu-item"}
            disabled={item.disabled}
            title={item.hint}
            onClick={() => {
              // Shut for good before the item runs: a dialog it opens finds
              // no menu left over, and the focus where the menu was opened.
              closeAndReturn();
              item.run();
            }}
          >
            {item.checked !== undefined && <span class="menu-check">{item.checked ? "●" : ""}</span>}
            {item.label}
            {item.shortcut && (
              <kbd class="menu-key" aria-hidden="true">
                {item.shortcut}
              </kbd>
            )}
          </button>
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
