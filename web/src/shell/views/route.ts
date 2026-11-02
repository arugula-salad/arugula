// Where a View is: `#view=<id>`, in place of the tab's panes.

import { useSubscribe } from "../../shared/ui/hooks";

export function viewRoute(): string | null {
  return /^#view=([a-z0-9]+)$/.exec(location.hash)?.[1] ?? null;
}

const subscribe = (fn: () => void) => {
  addEventListener("hashchange", fn);
  return () => removeEventListener("hashchange", fn);
};

/** The View shown now, kept up to date. */
export function useViewRoute(): string | null {
  useSubscribe(subscribe);
  return viewRoute();
}

export function openView(id: string) {
  location.hash = `view=${id}`;
}

export function closeView() {
  if (!viewRoute()) return;
  history.replaceState(null, "", location.pathname + location.search);
  dispatchEvent(new HashChangeEvent("hashchange"));
}
