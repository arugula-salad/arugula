// Whether this device is lent to agents (S33), in this browser's
// localStorage, and how many machines its hand reaches now. The hand
// itself (shell/hand.ts) runs on control's page, a daemon's own page and
// the app; its switch is in control's Sign-in and account and in the
// app's Settings › You, which can't import the shell, so both read and set
// it here, and the hand follows.

const KEY = "arugula-hand";
const listeners = new Set<() => void>();
let reached = 0;

export function lending(): boolean {
  try {
    return localStorage.getItem(KEY) === "on";
  } catch {
    return false;
  }
}

/** Turn lending on or off on this device; the hand starts or stops. */
export function setLend(on: boolean) {
  try {
    localStorage.setItem(KEY, on ? "on" : "off");
  } catch {
    // The setting just won't stick.
  }
  emit();
}

/** `#lend=on` or `#lend=off` in the address sets it too. */
export function lendFromAddress() {
  const m = /(?:^#|&)lend=(on|off)/.exec(location.hash);
  if (m) setLend(m[1] === "on");
}

/** Machines the hand reaches now (0 while it's off). */
export function lendReached(): number {
  return lending() ? reached : 0;
}

/** The hand says how many it reaches. */
export function setLendReached(n: number) {
  if (n === reached) return;
  reached = n;
  emit();
}

export function onLending(fn: () => void): () => void {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}

function emit() {
  for (const fn of [...listeners]) fn();
}
