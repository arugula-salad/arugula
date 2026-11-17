// A line at the bottom of the platform page that says what just happened
// or why not, for a few seconds (the shell's client has its own). One that
// can be taken back has its action beside the words (a deleted Doc's Undo,
// Arugula #1133), as the note after a move has (move/split-note.tsx), and
// stays longer for it.

/** What a toast offers beside its words: Undo. */
export interface ToastAction {
  label: string;
  run: () => void;
}

const listeners = new Set<() => void>();
let shown: { text: string; at: number; action?: ToastAction } | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;

export function toast(text: string, action?: ToastAction) {
  shown = { text, at: Date.now(), action };
  clearTimeout(timer);
  timer = setTimeout(dismiss, action ? 8000 : 4000);
  listeners.forEach((fn) => fn());
}

/** Gone now: its action was pressed. */
function dismiss() {
  clearTimeout(timer);
  shown = null;
  listeners.forEach((fn) => fn());
}

export const toasts = {
  shown: () => shown,
  /** Press its action, once: the toast goes. */
  act() {
    const a = shown?.action;
    dismiss();
    a?.run();
  },
  subscribe(fn: () => void) {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  },
};
