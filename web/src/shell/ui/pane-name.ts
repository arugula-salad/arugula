// What a pane is called where it's listed among others: the palette's
// jumps, closing a tab, share links.

import type { Client } from "../client";
import { cleanTitle } from "../../shared/titles";
import type { PaneInfo } from "../../shared/proto";

/** A pane by name: the title its program set, unless that's a shell's own
 * "user@host: dir"; else what runs; else its directory's last part. */
export function paneName(client: Client, i: PaneInfo): string {
  const title = cleanTitle(client.title(i.id));
  if (title && !/^[^\s@]+@[^\s:]+:/.test(title)) return title;
  const cwd = client.cwd(i.id);
  return i.current?.text || i.command || (cwd ? `shell in ${cwd.split("/").filter(Boolean).pop() ?? "/"}` : "") || `pane %${i.id}`;
}
