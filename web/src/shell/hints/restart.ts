// The restart hint's fact: a pane of the shown tab came back when the
// daemon started, and its process didn't (`PaneInfo.restored`): its
// policy (Pane settings › After a restart) said what ran instead.

import { paneIds, type Client } from "../client";
import type { Fact } from "./index";

export function restoredPane(client: Client): Fact | null {
  // The policy is the owner's to set.
  if (client.state?.roles) return null;
  const tab = client.tabView();
  if (!tab) return null;
  for (const id of paneIds(tab)) {
    const info = client.info(id);
    if (info?.type === "terminal" && info.restored) return { pane: id, args: { pane: id } };
  }
  return null;
}
