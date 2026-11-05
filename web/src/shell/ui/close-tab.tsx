// Closing a tab stops every pane in it (and deletes a VM tab's machine), so
// each way of closing one asks first (#753).

import { render } from "preact";
import { paneIds, tabLabel, type Client } from "../client";
import { ConfirmRemove } from "../../shared/ui/confirm";
import type { TabId } from "../../shared/proto";

/** Ask whether to close a tab; closing it is `go`'s. One dialog at a time. */
export function confirmCloseTab(client: Client, tabId: TabId) {
  const tab = client.tabView(tabId);
  if (!tab || document.querySelector("[data-close-tab-host]")) return;
  const panes = paneIds(tab).length;
  const machine = client.tabMachine(tabId);
  const host = document.createElement("div");
  host.dataset.closeTabHost = "";
  document.body.appendChild(host);
  const done = () => {
    render(null, host);
    host.remove();
  };
  render(
    <ConfirmRemove
      title="Close this tab?"
      label={machine ? "Close tab and machine" : "Close tab"}
      cancel={done}
      go={() => {
        done();
        client.intent({ op: "close_tab", tab: tabId });
      }}
    >
      <p>
        {tabLabel(client, tab)} has {panes === 1 ? "1 pane" : `${panes} panes`}. {panes === 1 ? "It stops" : "They stop"} when the tab closes.
        {machine && ` Its machine ${machine.name ?? machine.sprite} is deleted too.`}
      </p>
    </ConfirmRemove>,
    host,
  );
}
