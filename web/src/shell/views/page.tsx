// A View's page: `#view=<id>`, where the tab's panes are.

import { useEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../client";
import type { Cell } from "../ui/cells";
import { useSubscribe } from "../../shared/ui/hooks";
import { seenReady } from "../../shared/seen";
import { openSwarm } from "../swarm/route";
import { paneTitle } from "./links";
import { viewName } from "./model";
import { closeView } from "./route";
import { getView, noteLastView, renameView, subscribeViews } from "./store";
import { ViewSurface } from "./surface";

/** Name of a View, click to change. */
export function ViewName({ id }: { id: string }) {
  useSubscribe(subscribeViews);
  const [editing, setEditing] = useState(false);
  const doc = getView(id);
  const name = viewName(doc, paneTitle);
  if (editing) {
    return (
      <input
        class="view-rename"
        aria-label="View name"
        maxLength={40}
        value={doc?.n ?? name}
        ref={(el) => {
          if (el && document.activeElement !== el) {
            el.focus();
            el.select();
          }
        }}
        onBlur={(e) => {
          if (doc) renameView(id, (e.currentTarget as HTMLInputElement).value);
          setEditing(false);
        }}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
          if (e.key === "Escape") setEditing(false);
        }}
      />
    );
  }
  return (
    <button class="view-name" data-view-name title="Rename this view" disabled={!doc} onClick={() => setEditing(true)}>
      {name}
    </button>
  );
}

export function ViewPage({ id, client, cell, phone }: { id: string; client: Client; cell: Cell; phone: boolean }) {
  useSubscribe(subscribeViews);
  const area = useRef<HTMLDivElement>(null);
  const [ready, setReady] = useState(false);
  useEffect(() => setReady(true), []);
  useEffect(() => noteLastView(id), [id]);
  const doc = getView(id);
  return (
    <div class="view-page" data-view-page={id}>
      <div class="view-bar">
        <span class="view-mark">VIEW</span>
        <ViewName id={id} />
        <span class="view-bar-fill" />
        <button data-view-add title="Grab more blocks from the swarm" onClick={openSwarm}>
          {phone ? "Add" : "Add from the swarm"}
        </button>
        {phone && <button onClick={closeView}>Panes</button>}
      </div>
      <div class="view-area" ref={area}>
        {!doc && seenReady() && (
          <div class="view-empty" data-view-empty>
            <p>This view is empty.</p>
            <p>Hold a block in the swarm and fling it here. It keeps running where it lives.</p>
          </div>
        )}
      </div>
      {ready && (
        <ViewSurface
          key={id}
          id={id}
          area={area}
          main={client}
          cell={cell}
          phone={phone}
          sizing
          homeOf={() => {
            const r = document.querySelector('[data-rail-swarm], [data-place="swarm"]')?.getBoundingClientRect();
            return r ? { x: r.left, y: r.top, w: 24, h: 15 } : null;
          }}
        />
      )}
    </div>
  );
}
