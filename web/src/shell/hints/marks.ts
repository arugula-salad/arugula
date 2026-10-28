// The mark hint's fact: a command finished in a pane of the shown tab with
// shell integration, so its output has a green or red mark at the left
// edge (TerminalView's marks, from OSC 133). Right-clicking the mark copies
// its output or runs it again; the action opens that menu.

import { paneIds, type Client } from "../client";
import type { Fact } from "./index";

export function commandMarked(client: Client): Fact | null {
  const tab = client.tabView();
  if (!tab) return null;
  for (const id of paneIds(tab)) {
    if (client.info(id)?.type !== "terminal") continue;
    if (client.panes.get(id)?.view.marks.length) return { pane: id, args: { pane: id } };
  }
  return null;
}
