// The View stage in the swarm: where grabbed blocks land. It wakes when a block
// is picked up, and holds the same live leaves its View's page does.

import { useEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { Cell } from "../ui/cells";
import { useSubscribe } from "../../shared/ui/hooks";
import { openMenu } from "../../shared/ui/menu";
import { paneTitle } from "./links";
import { leaves, viewName, type Address, type Box, type ViewNode } from "./model";
import { ViewName } from "./page";
import { openView } from "./route";
import { getView, lastView, listViews, newViewId, noteLastView, subscribeViews } from "./store";
import { ViewSurface, type SurfaceApi } from "./surface";

export function ViewStage({
  main,
  cell,
  top,
  width,
  height,
  onReady,
  onClose,
  homeOf,
  onTree,
}: {
  main: Client;
  cell: Cell;
  /** Under the swarm's bar, whose height changes with what's in it. */
  top: number;
  width: number;
  height: number;
  onReady: (api: SurfaceApi | null) => void;
  onClose: () => void;
  homeOf: (a: Address) => Box | null;
  onTree: (t: ViewNode | null) => void;
}) {
  useSubscribe(subscribeViews);
  const [id, setId] = useState(() => lastView() ?? newViewId());
  const [awake, setAwake] = useState(false);
  const area = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);
  useEffect(() => setReady(true), []);
  useEffect(() => noteLastView(id), [id]);
  const doc = getView(id);
  useEffect(() => onTree(doc?.t ?? null), [JSON.stringify(doc?.t ?? null)]);
  const count = leaves(doc?.t ?? null).length;

  const pick = (e: MouseEvent) => {
    const views = listViews();
    openMenu(e, [
      { header: "Build into" },
      ...views.map(({ id: v, doc: d }) => ({ label: viewName(d, paneTitle), checked: v === id, run: () => setId(v) })),
      ...(views.length ? (["separator"] as const) : []),
      { label: "New view", run: () => setId(newViewId()) },
    ]);
  };

  return (
    <section class={`view-stage${awake ? " awake" : ""}`} data-view-stage={id} aria-label="View" style={{ "--stage-top": `${top}px`, "--stage-w": `${width}px`, "--stage-h": `${height}px` }}>
      <header class="view-bar">
        <span class="view-mark">VIEW</span>
        <ViewName id={id} />
        <button class="view-pick" data-view-pick aria-label="Choose a view" title="Choose a view" onClick={pick}>
          ▾
        </button>
        <span class="view-bar-fill" />
        <button data-view-open disabled={count === 0} title="Fill the window with this view" onClick={() => openView(id)}>
          Open
        </button>
        <button data-view-stage-close aria-label="Put the stage away" title="Put the stage away. The view stays." onClick={onClose}>
          ×
        </button>
      </header>
      <div class="view-area" ref={area}>
        {count === 0 && (
          <div class="view-empty" data-view-empty>
            <p>Drop blocks here.</p>
            <p>They keep running where they live.</p>
          </div>
        )}
      </div>
      {ready && <ViewSurface key={id} id={id} area={area} main={main} cell={cell} phone={false} sizing={false} homeOf={homeOf} onReady={onReady} onCarrying={setAwake} />}
    </section>
  );
}

