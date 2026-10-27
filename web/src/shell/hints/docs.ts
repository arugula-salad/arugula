// The Docs hint's fact: a Doc block is open in the shown tab.

import { paneIds, type Client } from "../client";
import type { Fact } from "./index";

export function docOpen(client: Client): Fact | null {
  if (client.state?.roles) return null;
  const tab = client.tabView();
  const id = tab && paneIds(tab).find((p) => client.info(p)?.type === "doc");
  return id === undefined || id === null ? null : { pane: id, args: { pane: id } };
}
