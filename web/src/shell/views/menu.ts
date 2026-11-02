// "Add to view" in a pane's menu: no dialog. Choosing a View adds the pane
// there; New view makes one and opens it.

import type { Client } from "../client";
import type { MenuItem } from "../../shared/ui/menu";
import type { PaneId } from "../../shared/proto";
import { directory } from "../hosts";
import { addressFor, paneTitle, viewsAvailable } from "./links";
import { viewName, type Address } from "./model";
import { openView } from "./route";
import { getView, listViews, newViewId, placeIn } from "./store";

/** Where there's room, as if the View filled a wide window. */
const SOME_BOX = { x: 0, y: 0, w: 1400, h: 800 };

/** Put a pane in a View from outside it. False: it holds no more. */
export function addToView(id: string, a: Address): boolean {
  return placeIn(id, a, SOME_BOX) !== null;
}

export function viewItems(client: Client, pane: PaneId): MenuItem[] {
  if (!viewsAvailable()) return [];
  const info = client.info(pane);
  // A remote pane is its host's: the View points there.
  const remote = info?.type === "remote" ? (client.blocks.get(pane)?.state as { host: string; pane: PaneId } | null) : null;
  const a = remote ? addressFor(remote.host, remote.pane) : addressFor(directory.current, pane);
  const make = () => {
    const id = newViewId();
    if (addToView(id, a)) openView(id);
  };
  const views = listViews();
  const into = (id: string) => {
    const name = viewName(getView(id), paneTitle);
    if (addToView(id, a)) client.toast(`Added to ${name}.`);
    else client.toast(`${name} holds no more blocks.`);
  };
  return [
    {
      label: "Add to view",
      submenu: [
        ...views.map(({ id, doc }): MenuItem => ({ label: viewName(doc, paneTitle), run: () => into(id) })),
        ...(views.length ? (["separator"] as MenuItem[]) : []),
        { label: "New view", run: make },
      ],
    },
  ];
}
