// Facts a hint learns outside the client's state (the answer to a
// request): saying one changed has the hints look again (./index).

const listeners = new Set<() => void>();

export function factChanged() {
  listeners.forEach((fn) => fn());
}

export function onFact(fn: () => void) {
  listeners.add(fn);
  return () => void listeners.delete(fn);
}
