// The tab's pane toolbar: the pane count and switcher, the layout presets,
// focus and + Pane. M110 (#1025) builds it; nothing shows it yet.

import type { Client } from "../../client";
import type { TabView } from "../../proto";

export function PaneToolbar(_: { client: Client; tab: TabView }) {
  return null;
}
