// What nvim's hints do (#1735; their facts and the help card are in
// ./nvim-facts, apart, so the registry (./list) reads them without
// reaching ./index through this file): add arugula.nvim, make nvim the
// Editor preference, open a tip's :help, show the help card.

import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import { opPath } from "../ops";
import { showHint, type Fact } from "./index";
import { setEditorPref } from "../open-in-editor";
import { local, NVIM_DOCS, NVIM_HELP, nvimRuns } from "./nvim-facts";
import { NVIM_TIPS, tipDocs } from "./nvim-tips";

export { NVIM_DOCS } from "./nvim-facts";
export { runsNvim } from "../open-in-editor";

/** Add arugula.nvim to the pane's nvim (`nvim_plugin.install`, #1733): it
 * loads there and joins as the pane, no restart. */
export async function addPlugin(client: Client, pane: PaneId) {
  try {
    const res = await client.request("POST", opPath("nvim_plugin.install", { id: pane }), {});
    if (!res.ok) {
      const why = (await res.json<{ error?: string }>().catch(() => null))?.error;
      client.toast(why ? `Couldn't add arugula.nvim: ${why}` : `Couldn't add arugula.nvim (${res.status})`);
    } else if (!(await res.json<{ joined?: boolean }>().catch(() => null))?.joined) {
      client.toast("arugula.nvim is added. nvim loads it the next time it starts");
    }
  } catch {
    client.toast("Couldn't add arugula.nvim");
  }
}

/** Open in editor opens files in nvim from now on: #1731's Editor
 * preference, the one Settings › Editor shows. */
export function useNvim(client: Client) {
  setEditorPref("nvim");
  client.toast("Open in editor opens files in nvim now. Settings › Editor changes it");
}

/** `:help <tag>` in the pane's nvim (`pane.nvim_help`, over #1730's RPC),
 * else the tag's page in nvim's online docs: an nvim Arugula can't reach
 * (another machine's, before 0.10, its server stopped). */
export async function openHelp(client: Client, pane: PaneId, help: string) {
  const url = tipDocs(NVIM_TIPS.find((t) => t.help === help) ?? { help });
  const info = client.info(pane);
  // Not this page's to reach: the docs at once, while it's still a click.
  if (!info?.editor || !local(client, pane, info)) {
    window.open(url, "_blank", "noopener");
    return;
  }
  const ok = await client.request("POST", opPath("pane.nvim_help", { id: pane }), { tag: help }).then(
    (r) => r.ok,
    () => false,
  );
  // A window opened after the answer may be blocked: then the toast says where.
  if (!ok && !window.open(url, "_blank", "noopener")) client.toast(`nvim didn't answer. Its help is at ${url}`);
}

// ---- the help card

/** Shows the help card over `pane` (at `section`), seen or not, from a
 * menu or the diff chip. Where nothing draws it (a phone), the docs page. */
export function openHelpCard(client: Client, pane: PaneId | undefined, section?: string) {
  const at = pane ?? nvimRuns(client)?.pane ?? client.active();
  const fact: Fact = { pane: at, args: { pane: at ?? "", ...(section ? { section } : {}) } };
  if (at === undefined || !showHint(client, NVIM_HELP, fact)) window.open(NVIM_DOCS, "_blank", "noopener");
}
