// Removing arugula.nvim from a machine (#1733), apart from ./nvim so
// Settings › Machines (../ui/machine-settings) reaches it without the
// hints' registry and the commands it brings.

import type { Client } from "../client";
import type { NvimPluginRemoved } from "../../shared/proto";
import { opPath } from "../ops";

/** Delete the copies of arugula.nvim Arugula installed on this machine
 * (`nvim_plugin.remove`, #1733): the machine menu's Remove arugula.nvim,
 * and Settings › Machines › Advanced. A copy a plugin manager installed
 * has no marker and stays. What happened, in a line. */
export async function removePluginSaid(client: Client): Promise<{ ok: boolean; said: string }> {
  try {
    const res = await client.request("DELETE", opPath("nvim_plugin.remove", {}));
    if (!res.ok) {
      const why = (await res.json<{ error?: string }>().catch(() => null))?.error;
      return { ok: false, said: why ? `Couldn't remove arugula.nvim: ${why}` : `Couldn't remove arugula.nvim (${res.status})` };
    }
    const removed = (await res.json<NvimPluginRemoved>().catch(() => null))?.removed ?? [];
    return {
      ok: true,
      said:
        removed.length === 0
          ? "No copy of arugula.nvim that Arugula installed is on this machine"
          : removed.length === 1
            ? `Removed arugula.nvim from ${removed[0]}`
            : `Removed arugula.nvim from ${removed.length} places: ${removed.join(", ")}`,
    };
  } catch {
    return { ok: false, said: "Couldn't remove arugula.nvim" };
  }
}
