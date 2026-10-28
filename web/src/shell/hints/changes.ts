// The Changes hint's fact: a commit or a checkout in a pane of the shown
// tab. The daemon reads the pane's repository's HEAD (`PaneInfo.branch`
// and `head`, arugula_mux::classify); the fact holds once either moved
// since this page first saw the pane in that repository. The owner's, as
// Changes is, and not where the tab shows a diff block already.

import { paneIds, type Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { seen } from "../../shared/seen";
import type { Fact } from "./index";

interface Repo {
  root: string;
  at: string;
  moved: boolean;
}

/** Each pane's repository as first seen, and whether HEAD moved since. */
const repos = new WeakMap<Client, Map<PaneId, Repo>>();

/** Every change the client hears, typing or not (the hints look only
 * when nobody types, and a `cd` then a `git commit` is quick), until the
 * hint is said. */
function note(client: Client, mine: Map<PaneId, Repo>) {
  if (seen("repo-changes")) return;
  for (const p of client.state?.panes ?? []) {
    const root = p.type === "terminal" ? p.project?.root : undefined;
    if (!root) continue;
    const at = JSON.stringify([p.branch ?? null, p.head ?? null]);
    const was = mine.get(p.id);
    // Another repository is a new start.
    if (was?.root !== root) mine.set(p.id, { root, at, moved: false });
    else if (was.at !== at) was.moved = true;
  }
}

export function repoMoved(client: Client): Fact | null {
  let mine = repos.get(client);
  if (!mine) {
    const m = (mine = new Map());
    repos.set(client, m);
    client.subscribe(() => note(client, m));
  }
  note(client, mine);
  if (client.state?.roles) return null;
  const tab = client.tabView();
  if (!tab) return null;
  const ids = paneIds(tab);
  if (ids.some((p) => client.info(p)?.type === "diff")) return null;
  const id = ids.find((p) => mine.get(p)?.moved && client.info(p)?.project?.root === mine.get(p)?.root);
  return id === undefined ? null : { pane: id, args: { pane: id } };
}
