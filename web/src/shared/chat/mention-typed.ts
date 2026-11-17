// Whether a message's text names a machine's agents with an `@mention`
// (#1167). Control reports a line for every machine it routed a post to,
// including ones the poster never typed (a thread's lead, a board's session),
// and the wire doesn't say which, so the text does.

import { machineHandle } from "./agents.ts";

/** True when `text` has `@machine` or `@machine/session` for the machine
 * named `name`, either as control resolves it (`aarons-mac-mini.local`) or as
 * the machine is shown (`Aarons-Mac-mini.local`), in any case. */
export function typesMachine(text: string, name: string): boolean {
  const words = new Set([machineHandle(name), name.toLowerCase()]);
  words.delete("");
  const low = text.toLowerCase();
  for (const w of words) {
    let from = 0;
    for (;;) {
      const at = low.indexOf(`@${w}`, from);
      if (at < 0) break;
      from = at + 1;
      const before = at > 0 ? low[at - 1] : "";
      const after = low[at + 1 + w.length] ?? "";
      // Not the tail of a longer word (an address), nor the head of a longer name.
      if (/[\p{L}\p{N}_]/u.test(before) || /[\p{L}\p{N}_-]/u.test(after) || (after === "." && /[\p{L}\p{N}_]/u.test(low[at + 2 + w.length] ?? ""))) continue;
      return true;
    }
  }
  return false;
}
