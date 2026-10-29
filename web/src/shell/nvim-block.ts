// nvim editor blocks (#1734): "New editor block" with nvim makes a
// terminal typed as an editor (its `app` is "nvim"), running nvim with
// arugula.nvim from the daemon. This page sends its palette to the daemon
// as nvim's colors (`nvimTheme`) when a block turns up and whenever the
// palette changes; the daemon keeps the latest and gives it to each
// block's plugin as it joins, so a block that starts with no page open
// still has them.

import { useEffect } from "preact/hooks";
import { onAppearance, palette } from "../shared/appearance";
import type { PaneId, PaneInfo } from "../shared/proto";
import type { Client } from "./client";
import { nvimTheme } from "./editor-theme";
import { OPS, opPath } from "./ops";

/** An nvim editor block, on this machine (one on a VM has no plugin to
 * take colors). */
export const isNvimBlock = (p: PaneInfo | null | undefined): boolean => p?.app === "nvim";

/** Sends the palette to `client`'s daemon for its nvim blocks: once for
 * each block that turns up, and on each palette change while there's one. */
export function watchNvimBlocks(client: Client): () => void {
  const seen = new Set<PaneId>();
  let timer: ReturnType<typeof setTimeout> | undefined;
  const blocks = () => (client.state?.panes ?? []).filter((p) => isNvimBlock(p) && p.host == null);
  const send = () => {
    timer = undefined;
    // The owner's: a guest's page has its own palette, not the machine's.
    if (client.state?.roles || !client.connected) return;
    void client.request(OPS["nvim_block.theme"].method, opPath("nvim_block.theme"), nvimTheme(palette())).catch(() => {});
  };
  const soon = () => {
    if (timer === undefined) timer = setTimeout(send, 150);
  };
  const look = () => {
    let fresh = false;
    for (const p of blocks()) {
      if (!seen.has(p.id)) {
        seen.add(p.id);
        fresh = true;
      }
    }
    if (fresh) soon();
  };
  look();
  const offs = [
    client.subscribe(look),
    onAppearance(() => {
      if (blocks().length) soon();
    }),
  ];
  return () => {
    offs.forEach((f) => f());
    if (timer !== undefined) clearTimeout(timer);
  };
}

/** Keeps the shown machine's nvim blocks in the page's colors; once, in
 * the workspace. */
export function NvimBlockColors({ client }: { client: Client }) {
  useEffect(() => watchNvimBlocks(client), [client]);
  return null;
}
