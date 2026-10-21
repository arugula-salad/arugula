// A line at the bottom of the platform page that says what just happened
// or why not, for a few seconds (the shell's client has its own).

const listeners = new Set<() => void>();
let shown: { text: string; at: number } | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;

export function toast(text: string) {
  shown = { text, at: Date.now() };
  clearTimeout(timer);
  timer = setTimeout(() => {
    shown = null;
    listeners.forEach((fn) => fn());
  }, 4000);
  listeners.forEach((fn) => fn());
}

export const toasts = {
  shown: () => shown,
  subscribe(fn: () => void) {
    listeners.add(fn);
    return () => void listeners.delete(fn);
  },
};
