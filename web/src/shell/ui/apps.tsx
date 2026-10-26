// Opening an app's box beside your terminals (M35), by its address: the
// box signs you in with your Arugula account, so nothing is asked of the
// daemon but the block. The apps open here already are offered first, and
// picking one goes to its block. Opened from a pane's menu, the `+`
// button's menu and the phone's sheet. (Native #206 retired the studio
// login that listed your apps by name.)

import type { Client } from "../client";
import type { OpenRequest, OpenResponse, PaneId } from "../../shared/proto";
import { askText, type PromptChoice } from "../../shared/ui/menu";

export interface AppsWhere {
  /** Split this block, else a new tab in `session`. */
  split?: PaneId;
  session?: number;
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}

/** An address as the daemon keeps it: its origin. */
function originOf(url: string): string | null {
  try {
    return new URL(/^[a-z]+:\/\//i.test(url) ? url : `https://${url}`).origin;
  } catch {
    return null;
  }
}

/** The app blocks open here, by their box's origin. */
function openApps(client: Client): Map<string, { id: PaneId; name: string }> {
  const out = new Map<string, { id: PaneId; name: string }>();
  for (const [id, b] of client.blocks) {
    const s = b.state as { box_url?: unknown; title?: string | null; app?: string | null } | null;
    if (typeof s?.box_url !== "string" || out.has(s.box_url)) continue;
    out.set(s.box_url, { id, name: s.title ?? s.app ?? hostOf(s.box_url) });
  }
  return out;
}

/** Ask for an app's address and open its box, or go to the block that has
 * it already. */
export async function pickApp(client: Client, where: AppsWhere) {
  const open = openApps(client);
  const choices: PromptChoice[] = [...open].map(([url, a]) => ({
    value: url,
    label: a.name,
    hint: a.name === hostOf(url) ? undefined : hostOf(url),
    title: url,
  }));
  const typed = (await askText("Open an app", "", "its address, like https://pinboard.example", Promise.resolve(choices)))?.trim();
  if (!typed) return;
  const had = open.get(originOf(typed) ?? "");
  if (had) {
    client.setActive(had.id);
    return;
  }
  await openApp(client, typed, where);
}

/** Open the box at `url` as a block; the new block, or null. */
export async function openApp(client: Client, url: string, where: AppsWhere): Promise<PaneId | null> {
  const res = await client.request("POST", "/api/blocks", {
    type: "app",
    config: { box_url: url },
    split: where.split ?? null,
    session: where.session === undefined ? null : String(where.session),
    from_pane: where.split ?? null,
  } satisfies OpenRequest);
  if (!res.ok) {
    const e = await res.json<{ error?: string }>().catch(() => ({ error: undefined }));
    client.toast(`couldn't open ${url}: ${e.error ?? res.status}`);
    return null;
  }
  return (await res.json<OpenResponse>()).block;
}
