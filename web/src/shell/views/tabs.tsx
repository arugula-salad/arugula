// Views beside the tabs: each a tab of its own that isn't any machine's.

import { useSubscribe } from "../../shared/ui/hooks";
import { getFleet } from "../ui/hosts";
import { openMenu } from "../../shared/ui/menu";
import { paneTitle, viewsAvailable } from "./links";
import { leaves, viewName } from "./model";
import { closeView, openView, useViewRoute, viewRoute } from "./route";
import { deleteView, listViews, subscribeViews } from "./store";
import { useEffect } from "preact/hooks";

/** What the tab bar shows for each View. */
export function ViewTabs() {
  useSubscribe(subscribeViews);
  const fleet = getFleet();
  useSubscribe((fn) => fleet?.subscribe(fn) ?? (() => {}));
  const at = useViewRoute();
  // A View that went (deleted on another device) can't stay shown.
  const views = listViews();
  useEffect(() => {
    void at;
  }, [at]);
  if (!viewsAvailable() || views.length === 0) return null;
  return (
    <div class="view-tabs" role="tablist" aria-label="Views">
      {views.map(({ id, doc }) => {
        const name = viewName(doc, paneTitle);
        return (
          <button
            key={id}
            role="tab"
            aria-selected={at === id}
            class={at === id ? "view-tab selected" : "view-tab"}
            data-view-tab={id}
            title={`${name}: a view of ${leaves(doc.t).length} block${leaves(doc.t).length === 1 ? "" : "s"}`}
            onClick={() => openView(id)}
            onContextMenu={(e) => {
              e.preventDefault();
              openMenu(e, [
                { header: name },
                { label: "Open", run: () => openView(id) },
                {
                  label: "Close view",
                  hint: "The panes in it keep running in their tabs",
                  run: () => {
                    deleteView(id);
                    if (viewRoute() === id) closeView();
                  },
                },
              ]);
            }}
          >
            <span class="view-tab-mark" aria-hidden="true">
              ⧉
            </span>
            <span class="view-tab-name">{name}</span>
          </button>
        );
      })}
    </div>
  );
}
