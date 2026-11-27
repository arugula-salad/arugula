// Tabs and panes (design lift §4.1, part 11; Arugula #1497): the one way a
// pane's view is drawn in a slot, the grid's (PaneSlot), a board drawer's
// (LiveSlot, LivePane) and the agent panel's (WorkSlot). The view moves in
// rather than being made again, so moving a pane never restarts or redraws
// it; when the slot goes, it goes back where it was (`home`) or to parking.

import { useLayoutEffect } from "preact/hooks";

/** What a slot draws: a block's view (a terminal's, an app's). */
export interface SlotView {
  host: HTMLElement;
  setVisible(on: boolean): void;
}

let parked: HTMLElement | null = null;

/** Where views not in a slot live: off the page but still alive. */
export function parking(): HTMLElement {
  if (!parked) {
    parked = document.createElement("div");
    parked.id = "parking";
    document.body.appendChild(parked);
  }
  return parked;
}

/** Move `view` into `ref`'s element while both are there. `home`: when the
 * slot goes, put it back where it was before (a grid slot that's still on
 * the page), not in parking. */
export function useSlot(view: SlotView | undefined, ref: { current: HTMLElement | null }, home = false) {
  useLayoutEffect(() => {
    const slot = ref.current;
    if (!view || !slot) return;
    const was = view.host.parentElement;
    slot.appendChild(view.host);
    view.setVisible(true);
    return () => {
      // Another slot took it meanwhile: it's there.
      if (view.host.parentElement !== slot) return;
      if (home && was && was !== parked && was.isConnected) return void was.appendChild(view.host);
      parking().appendChild(view.host);
      view.setVisible(false);
    };
  }, [view]);
}
