// Which way each port or editor block on this page is shown (M50, #150):
// through control, straight to the daemon or relayed (the frame says which
// way its channel actually went), or from the machine's own block site
// after control's way didn't load. The block shows its own; the host menu
// sums them up, in the words it uses for the machine's own connection.

import type { PaneId } from "../../shared/proto";
import type { Route } from "../block/channel.ts";

export type BlockRoute = Route | "own site";

const routes = new Map<PaneId, BlockRoute>();

/** Set (or, with null, forget) block `id`'s route. */
export function setBlockRoute(id: PaneId, r: BlockRoute | null) {
  if (r === null) routes.delete(id);
  else routes.set(id, r);
}

/** The host menu's line for the blocks shown here: "Blocks: relayed", or
 * "Blocks: 1 direct, 2 relayed"; null when none is shown through control. */
export function blockRoutesLabel(all: Iterable<BlockRoute> = routes.values()): string | null {
  const n = new Map<BlockRoute, number>();
  for (const r of all) n.set(r, (n.get(r) ?? 0) + 1);
  if (!n.size) return null;
  if (n.size === 1) {
    const [r] = n.keys();
    return `Blocks: ${r === "own site" ? "the machine's own block site" : r}`;
  }
  const order: BlockRoute[] = ["direct", "relayed", "own site"];
  return `Blocks: ${order
    .filter((r) => n.has(r))
    .map((r) => `${n.get(r)} ${r === "own site" ? "on its own block site" : r}`)
    .join(", ")}`;
}

/** The block's own pill's tooltip. */
export function routeTitle(r: BlockRoute): string {
  if (r === "relayed") return "Through Arugula control's relay (end to end encrypted)";
  if (r === "direct") return "Straight to the machine (end to end encrypted)";
  return "Via the machine's own block site: this browser couldn't open it through Arugula control";
}
