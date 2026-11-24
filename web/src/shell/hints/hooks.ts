// The hooks offer's fact (https://app.arugula.io/issues/iss_fa949b979b607ef8f0414cbe674ab33b):
// Claude Code in a terminal of the shown tab waits for an answer (a
// permission prompt), none of Arugula's hooks has spoken from it
// (`PaneInfo.unhooked`, the daemon's word, which is Claude Code's alone, so
// never Codex), and Claude Code's settings on the owner's own machine lack
// the hooks (`hooksInstalled`, asked once a page, only while the hint is
// unseen with Tips on: ./index asks only then). Asked as the page's
// machine comes, before any Claude Code: hints aren't asked while someone
// types, and a prompt often comes seconds after the keys that started it,
// so an answer asked for only then would come after a hint whose fact
// needs none (the bell's toast) took the moment.

import { paneIds, type Client } from "../client";
import { hooksInstalled } from "../ui/agents-setup";
import type { Fact } from "./index";
import { factChanged } from "./facts";

/** Whether the hooks are missing: unknown until the daemon answers. */
let missing: boolean | "asking" | undefined;

export function claudeWaiting(client: Client): Fact | null {
  // The owner's, on the machine itself: only its daemon can write them.
  if (!client.state || client.state.roles || client.e2e) return null;
  if (missing === undefined) {
    missing = "asking";
    void hooksInstalled().then((has) => {
      missing = !has;
      factChanged();
    });
  }
  if (missing !== true) return null;
  const tab = client.tabView();
  if (!tab) return null;
  for (const id of paneIds(tab)) {
    const info = client.info(id);
    if (info?.type === "terminal" && info.unhooked && info.attention === "needs_input" && !info.ask && info.host == null && !client.machine(id)) {
      return { pane: id, args: { pane: id } };
    }
  }
  return null;
}
