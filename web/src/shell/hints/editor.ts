// The editor hint's fact: `code` was the last command line in a pane of the
// shown tab, on a machine whose blocks can show: VS Code opens beside it,
// in its directory. The owner's, as Open in editor is, and not where the
// tab shows an editor block already. Never for a pane running vim or nvim:
// VS Code isn't what they're using, and nvim's own chips (./nvim, #1735)
// are said there instead.

import { paneIds, type Client } from "../client";
import type { Fact } from "./index";

const VIM = /^(?:n?vim?|vi)$/;

export function editorStarted(client: Client): Fact | null {
  if (client.state?.roles || !client.showsBlocks()) return null;
  const tab = client.tabView();
  if (!tab) return null;
  const ids = paneIds(tab);
  if (ids.some((p) => client.info(p)?.type === "editor")) return null;
  for (const id of ids) {
    const info = client.info(id);
    if (info?.type !== "terminal" || info.editor) continue;
    const prog = info.command?.trim().split(/\s+/)[0]?.split("/").pop() ?? "";
    if (VIM.test(prog)) continue;
    const typed = (info.current ?? info.last)?.text?.trim() ?? "";
    if (/^code(?:\s|$)/.test(typed)) return { pane: id, args: { pane: id } };
  }
  return null;
}
