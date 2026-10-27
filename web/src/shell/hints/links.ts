// The PR link hint's fact: a pane of the shown tab printed a link to a
// pull request or an issue (`gh pr create`'s). The daemon sees the output
// and says so in `PaneInfo.link` (arugula_mux::osc::forge_link); the
// client says which of those it opens as blocks (`Client.forgeLink`).

import { paneIds, type Client } from "../client";
import type { Fact } from "./index";

export function printedLink(client: Client): Fact | null {
  // A guest's clicks open links in the browser (Client.openForgeLink).
  if (client.state?.roles) return null;
  const tab = client.tabView();
  if (!tab) return null;
  for (const id of paneIds(tab)) {
    const url = client.info(id)?.link;
    if (url && client.forgeLink(url)) return { pane: id, args: { pane: id, url } };
  }
  return null;
}
