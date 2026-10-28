// The attach hint's fact: a path pasted into a pane (a file's, copied from
// this computer: it means nothing on the pane's machine), or files dragged
// over one. The terminal says so (TerminalView.onOffered); the fact holds
// for a minute after, on that pane, for someone who may type in it.

import type { Client } from "../client";
import type { PaneId } from "../../shared/proto";
import type { Fact } from "./index";
import { factChanged } from "./facts";

const HOLD_MS = 60_000;

let last: { pane: PaneId; at: number } | null = null;

/** One line that looks like a path: `/…`, `~/…`, `C:\…`, a `file://` URL. */
export function pathLike(text: string): boolean {
  const t = text.trim().replace(/^(['"])(.*)\1$/, "$2");
  if (!t || t.length > 1024 || /[\r\n]/.test(t)) return false;
  return /^(?:~?\/[^/\s]|[A-Za-z]:\\[^\\]|file:\/\/)/.test(t) && !/\s{2}/.test(t);
}

/** The terminal of `pane` was offered text, or files over it. */
export function offered(pane: PaneId, what: { text: string } | { files: true }) {
  if ("text" in what && !pathLike(what.text)) return;
  // A drag says so many times a second.
  if (last?.pane === pane && Date.now() - last.at < 1000) return;
  last = { pane, at: Date.now() };
  factChanged();
  setTimeout(factChanged, HOLD_MS + 50);
}

export function pathOffered(client: Client): Fact | null {
  if (!last || Date.now() - last.at > HOLD_MS) return null;
  const { pane } = last;
  if (client.tabOfPane(pane)?.id !== client.tabView()?.id || !client.mayType(pane) || !client.panes.has(pane)) return null;
  return { pane, args: { pane } };
}
