// The tab's head line (M110, #1025; Lift 6a, v6's Machines): what the tab
// is (its state, name, machine and kind), then above the panes and outside
// their cells Tamed and Grid, and + Pane. Each layout is a daemon intent
// (`arrange`), so it's everyone's, and a refusal says why here. Columns, the
// pane switcher and Focus went in Lift 6a (#1483): Go to… and the keys have
// the panes, and Tamed has one large.

import { useEffect, useState } from "preact/hooks";
import { paneIds, tabLabel, type Client } from "../../client";
import { directory } from "../../hosts";
import { Mark, type State } from "../../../shared/kit/state-mark";
import type { Preset, TabView } from "../../../shared/proto";
import { keyHint } from "../../../shared/keys";
import { arrange, arrangeRefusal, layoutRefusal, PRESETS } from "../panes/layout";
import { AddPaneDrawer, addPane, useAddPane } from "./add-pane";
import { paneType } from "./pane-header";
import { PlusIcon } from "../../../shared/ui/icons";

export function PaneToolbar({ client, tab }: { client: Client; tab: TabView }) {
  const ids = paneIds(tab);
  const count = ids.length;
  const why = arrangeRefusal(client, tab);
  const open = useAddPane();
  // Someone who only watches (a viewer, a read-only link's page) adds no
  // panes: no greyed + Pane for them (UI audit #197).
  const watching = client.role(client.sessionOfTab(tab.id) ?? null) === "viewer";
  // The daemon's no (the tab too small for a preset), until the layout
  // changes or a while passes.
  const [note, setNote] = useState<string | null>(null);
  useEffect(() => setNote(null), [tab.id, count, tab.focus]);
  useEffect(() => {
    if (!note) return;
    const t = window.setTimeout(() => setNote(null), 8000);
    return () => clearTimeout(t);
  }, [note]);
  const refused = (m: string) => setNote(m.charAt(0).toUpperCase() + m.slice(1));

  return (
    <div class="pane-toolbar" data-pane-toolbar role="toolbar" aria-label="Panes">
      <span class="pt-head" data-tab-head={tab.id}>
        <Mark state={headMark(client, ids)} />
        <b class="pt-title">{tabLabel(client, tab)}</b>
        <span class="m-meta">{[directory.current, paneType(client.info(client.active(tab.id) ?? ids[0]!))].filter(Boolean).join(" · ")}</span>
      </span>
      <div class="pt-presets" role="group" aria-label="Arrange">
        {PRESETS.filter((p) => p.preset !== "columns").map(({ preset, label }) => (
          <button
            key={preset}
            class="pt-preset bar-btn"
            data-preset={preset}
            disabled={!!why}
            // Its name at any width (the label hides when narrow); why
            // it's off is the title's (UI audit #185).
            aria-label={label}
            title={why ?? hint(preset)}
            onClick={() => {
              setNote(null);
              arrange(client, tab, preset, refused);
            }}
          >
            <Thumb preset={preset} />
            <span class="pt-label">{label}</span>
          </button>
        ))}
      </div>
      {note && (
        <span class="pt-note" role="status" data-pane-toolbar-note>
          {note}
        </span>
      )}
      <span class="pt-fill" />
      {!watching && (
        <button
          class={`pt-add bar-btn${open ? " on" : ""}`}
          data-add-pane
          aria-expanded={open}
          disabled={!!layoutRefusal(client, tab)}
          title={layoutRefusal(client, tab) ?? `Add a pane beside the active one (${keyHint("pane.add")})`}
          onClick={() => addPane.toggle()}
        >
          <PlusIcon size="sm" />
          Pane
          {keyHint("pane.add") && <kbd>{keyHint("pane.add")}</kbd>}
        </button>
      )}
      {open && !watching && <AddPaneDrawer client={client} tab={tab} />}
    </div>
  );
}

/** The tab's state: a pane asks you, one works, or it's quiet. */
function headMark(client: Client, ids: number[]): State {
  const infos = ids.map((id) => client.info(id));
  if (infos.some((i) => i?.attention === "needs_input")) return "asks";
  return infos.some((i) => i?.attention === "working") ? "working" : "idle";
}

function hint(p: Preset): string {
  return p === "grid" ? "Grid: rows of equal panes" : "Tamed: the active pane large, the rest stacked beside it";
}

/** What each preset looks like, drawn small. */
function Thumb({ preset }: { preset: Preset }) {
  const r = (x: number, y: number, w: number, h: number) => <rect x={x} y={y} width={w} height={h} rx="1" />;
  return (
    <svg class="pt-thumb" width="18" height="13" viewBox="0 0 18 13" aria-hidden="true">
      {preset === "grid"
        ? [r(0.5, 0.5, 8, 5.5), r(9.5, 0.5, 8, 5.5), r(0.5, 7, 8, 5.5), r(9.5, 7, 8, 5.5)]
        : [r(0.5, 0.5, 10, 12), r(11.5, 0.5, 6, 3.5), r(11.5, 4.75, 6, 3.5), r(11.5, 9, 6, 3.5)]}
    </svg>
  );
}

