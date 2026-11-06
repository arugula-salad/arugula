// Closing a tab stops every pane in it (and deletes a VM tab's machine), so
// each way of closing one asks first (#753). The phone's Close pane asks the
// same way (#709): a tap in its sheet is easy to make by accident.

import { render } from "preact";
import { paneIds, tabLabel, type Client } from "../client";
import { ConfirmRemove } from "../../shared/ui/confirm";
import type { PaneId, TabId } from "../../shared/proto";
import { paneName } from "./shell/overview";

/** Draw `ask` over the page, in a host of its own that goes with it. One dialog at a time. */
function confirm(ask: (done: () => void) => preact.ComponentChildren) {
  if (document.querySelector("[data-close-tab-host]")) return;
  const host = document.createElement("div");
  host.dataset.closeTabHost = "";
  document.body.appendChild(host);
  const done = () => {
    render(null, host);
    host.remove();
  };
  render(ask(done), host);
}

/** Ask whether to close a tab; closing it is `go`'s. */
export function confirmCloseTab(client: Client, tabId: TabId) {
  const tab = client.tabView(tabId);
  if (!tab) return;
  const panes = paneIds(tab).length;
  const machine = client.tabMachine(tabId);
  confirm((done) => (
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
    </ConfirmRemove>
  ));
}

/** Ask whether to close a pane; a pane with a process running says so. */
export function confirmClosePane(client: Client, pane: PaneId) {
  const info = client.info(pane);
  if (!info) return;
  const running = !!info.command || !!info.current;
  confirm((done) => (
    <ConfirmRemove
      title="Close this pane?"
      label="Close pane"
      cancel={done}
      go={() => {
        done();
        client.intent({ op: "close_pane", pane });
      }}
    >
      <p>
        {paneName(client, info)}
        {running ? ` is running ${info.command ?? info.current?.text ?? "a process"}. It stops when the pane closes.` : " will close."}
      </p>
    </ConfirmRemove>
  ));
}
