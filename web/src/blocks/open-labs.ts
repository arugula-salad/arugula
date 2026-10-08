// Opening a Labs block, and asking whether a directory is a workspace, need
// none of the block's code: each is a request to the daemon. They live here,
// in the main bundle, so a menu can offer them without loading Labs (the
// blocks themselves are drawn by ./lazy and the Labs chunk).

import { useEffect, useState } from "preact/hooks";
import type { Client } from "../client";
import type { PaneId } from "../proto";

/** Directories known to hold a declaration, or not, by the pane whose
 * host they're on. */
const known = new Map<string, boolean>();

/** Whether to offer `dir`, where `pane` runs (its machine, or this host),
 * as a chant workspace: it holds a `chant.workspace.json` or `.jsonc`. Only
 * for the offer: the block asks chant, which also looks above. */
export async function isWorkspace(client: Client, pane: PaneId, dir: string): Promise<boolean> {
  const key = `${client.machine(pane)?.id ?? "here"}:${dir}`;
  const had = known.get(key);
  if (had !== undefined) return had;
  try {
    let yes = false;
    for (const file of ["chant.workspace.json", "chant.workspace.jsonc"]) {
      const path = `${dir.replace(/\/$/, "")}/${file}`;
      if ((yes = (await client.request("GET", `/api/fs/stat?pane=${pane}&path=${encodeURIComponent(path)}`)).ok)) break;
    }
    known.set(key, yes);
    return yes;
  } catch {
    return false;
  }
}

/** Whether the directory `pane` is in is a chant workspace, looked up when
 * it changes (the owner's only: files are theirs), for menus to offer
 * "Open as workspace". */
export function useWorkspaceDir(client: Client, pane: PaneId, dir: string | null | undefined): boolean {
  const [yes, setYes] = useState(false);
  useEffect(() => {
    setYes(false);
    if (!dir || client.state?.roles) return;
    let live = true;
    void isWorkspace(client, pane, dir).then((v) => live && setYes(v));
    return () => {
      live = false;
    };
  }, [client, pane, dir]);
  return yes;
}

/** A workspace block for `root`, beside `from` (on its host), or in a new
 * tab here. */
export function openWorkspace(client: Client, root: string, from?: PaneId, env = "local") {
  void client.openBlock(
    { type: "workspace", config: { root, env }, ...(from !== undefined ? { split: from, from_pane: from } : { local: true }) },
    "couldn't open the workspace",
  );
}

/** "Fountain agents…" (or, M45b, "Fountain runner…"): the block beside
 * `split`, or in a new tab of `session`. */
export async function openFountain(client: Client, where: { split?: PaneId; session?: number }, view: "catalog" | "runner" = "catalog") {
  const place = where.split !== undefined ? { split: where.split, from_pane: where.split } : { session: where.session !== undefined ? String(where.session) : undefined };
  const failure = view === "runner" ? "couldn't open the Fountain runner" : "couldn't open the Fountain catalog";
  await client.openBlock({ type: "fountain", config: { view }, local: true, ...place }, failure);
}
