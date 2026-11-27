// A pane of this machine drawn outside the grid, in a fixed slot: a board
// row's drawer, its Live session (`PlatformApi.LivePane`, Arugula
// iss_c62e00eee3b4b953ff054d2951827de8). The block's view moves in as it
// does into its PaneSlot, and goes back where it was when the slot closes:
// its slot in the grid, or parking when its tab isn't shown. It keeps its
// size in cells; the slot scrolls what doesn't fit.

import { useRef } from "preact/hooks";
import type { Client } from "../../client";
import type { PaneId } from "../../../shared/proto";
import { useSubscribe } from "../../../shared/ui/hooks";
import { useSlot } from "../../../shared/kit/panes/slot";

/** The pane in a slot, with Hand off to me: take control (#118), so the
 * reader's typing reaches it, and put the keys in it. Nothing when this
 * page doesn't have the pane. */
export function LiveSlot({ client, pane }: { client: Client; pane: PaneId }) {
  useSubscribe((fn) => client.subscribe(fn));
  const ref = useRef<HTMLDivElement>(null);
  const view = client.viewOf(pane);

  useSlot(view, ref, true);

  if (!view) return null;
  const mine = client.info(pane)?.driver?.who === client.me();
  const watching = !!client.state?.roles && client.role() === "viewer";
  return (
    <>
      <div ref={ref} class="live-slot" data-live-slot={pane} />
      {!watching && (
        <div class="board-line">
          <button
            type="button"
            class="primary small"
            data-live-hand-off
            disabled={mine}
            title={mine ? "You have control: your typing reaches it" : "Take control of this pane: your typing reaches it, and the agent's driver hands it to you"}
            onClick={() => {
              client.paneOp(pane, { op: "take_control" });
              view.focus();
            }}
          >
            Hand off to me
          </button>
        </div>
      )}
    </>
  );
}
