// The private pane hint's fact: the Share dialog warned that a pane may
// show secrets (the daemon's guess, `/api/sessions/{id}/secrets`), and,
// with the dialog closed, that pane is in the shown tab and still not
// private. The owner's: Pane › Share › Private hides it from everyone
// they share with.

import { paneIds, type Client } from "../client";
import type { PaneId } from "../../shared/proto";
import type { Fact } from "./index";
import { factChanged } from "./facts";

const warned = new Set<PaneId>();
/** Share dialogs open now. */
let open = 0;

/** The Share dialog warned about `panes` (ui/people's ShareDialog). */
export function secretsWarned(panes: PaneId[]) {
  for (const p of panes) warned.add(p);
  if (panes.length) factChanged();
}

/** A Share dialog opened, until the returned function is called: the
 * hint waits until it's closed. */
export function shareOpen(): () => void {
  open++;
  factChanged();
  return () => {
    open--;
    factChanged();
  };
}

export function secretPane(client: Client): Fact | null {
  if (open || !warned.size || client.state?.roles) return null;
  const tab = client.tabView();
  const id = tab && paneIds(tab).find((p) => warned.has(p) && client.info(p)?.type === "terminal" && !client.info(p)?.private);
  return id === undefined || id === null ? null : { pane: id, args: { pane: id } };
}
