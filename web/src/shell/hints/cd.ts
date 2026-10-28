// The directory hint's fact: `cd` typed in a fresh pane (one made since
// this page started watching), as the shell integration reports the
// command line: the picker goes there in a keystroke. It holds while that
// `cd` is the pane's last command.

import { paneIds, type Client } from "../client";
import type { PaneId } from "../../shared/proto";
import type { Fact } from "./index";

/** The panes there were when the client was first asked about. */
const before = new WeakMap<Client, Set<PaneId>>();

export function cdTyped(client: Client): Fact | null {
  const state = client.state;
  if (!state) return null;
  let old = before.get(client);
  if (!old) before.set(client, (old = new Set(state.panes.map((p) => p.id))));
  const tab = client.tabView();
  if (!tab) return null;
  for (const id of paneIds(tab)) {
    const info = client.info(id);
    if (old.has(id) || info?.type !== "terminal" || !client.mayType(id)) continue;
    const text = (info.current ?? info.last)?.text?.trim();
    if (text && /^cd(?:\s|$)/.test(text)) return { pane: id, args: { pane: id } };
  }
  return null;
}
