// The tab's pane toolbar (M110, #1025), above the panes and outside their
// cells: the pane count and switcher, the layout presets, Focus and
// + Pane. Every button is a daemon intent (`arrange`, `focus`), so the
// layout it makes is everyone's, and a refusal says why here.

import { useEffect, useState } from "preact/hooks";
import { paneIds, type Client } from "../../client";
import type { Preset, TabView } from "../../../shared/proto";
import { keyHint } from "../keys";
import { openMenu, type MenuItem } from "../../../shared/ui/menu";
import { arrange, arrangeRefusal, goToPane, layoutRefusal, PRESETS, toggleFocus } from "../panes/layout";
import { AddPaneDrawer, addPane, useAddPane } from "./add-pane";
import { paneStatus, paneType } from "./pane-header";
import { PlusIcon } from "./icons";

export function PaneToolbar({ client, tab }: { client: Client; tab: TabView }) {
  const ids = paneIds(tab);
  const count = ids.length;
  const focused = tab.focus !== null;
  const why = arrangeRefusal(client, tab);
  const focusWhy = layoutRefusal(client, tab) ?? (count < 2 && !focused ? "There's only one pane" : null);
  const open = useAddPane();
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

  const switcher = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const active = client.active(tab.id);
    const items: MenuItem[] = [
      { header: focused ? "Panes (one shown)" : "Panes in this tab" },
      ...ids.map((id, i): MenuItem => {
        const info = client.info(id);
        const s = paneStatus(info);
        return {
          label: `${i + 1}  ${client.title(id) || info?.command || `pane %${id}`} · ${paneType(info)}${s.word && s.word !== "idle" ? ` · ${s.word}` : ""}`,
          checked: id === active,
          shortcut: i < 9 ? keyHint("pane.jump")?.replace("1–9", String(i + 1)) : undefined,
          run: () => goToPane(client, id),
        };
      }),
    ];
    openMenu({ clientX: r.left, clientY: r.bottom + 4, preventDefault() {} }, items);
  };

  return (
    <div class="pane-toolbar" data-pane-toolbar role="toolbar" aria-label="Panes">
      <button class="pt-count" data-pane-count={count} aria-haspopup="menu" title="Go to a pane" onClick={switcher}>
        <strong>{count}</strong> {count === 1 ? "pane" : "panes"}
        <span class="pt-caret" aria-hidden="true">
          ▾
        </span>
      </button>
      <span class="pt-sep" aria-hidden="true" />
      <div class="pt-presets" role="group" aria-label="Arrange">
        {PRESETS.map(({ preset, label }) => (
          <button
            key={preset}
            class="pt-preset"
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
      <button
        class={`pt-focus${focused ? " on" : ""}`}
        data-pane-focus
        aria-pressed={focused}
        disabled={!!focusWhy}
        title={focusWhy ?? `${focused ? "Show every pane again" : "Fill the tab with the active pane"} (${keyHint("pane.focus")})`}
        onClick={() => toggleFocus(client, tab, undefined, refused)}
      >
        <FocusIcon on={focused} />
        {focused ? "Restore" : "Focus"}
      </button>
      {note && (
        <span class="pt-note" role="status" data-pane-toolbar-note>
          {note}
        </span>
      )}
      <span class="pt-fill" />
      <button
        class={`pt-add${open ? " on" : ""}`}
        data-add-pane
        aria-expanded={open}
        disabled={!!layoutRefusal(client, tab)}
        title={layoutRefusal(client, tab) ?? `Add a pane beside the active one (${keyHint("pane.add")})`}
        onClick={() => addPane.toggle()}
      >
        <PlusIcon size={14} />
        Pane
        {keyHint("pane.add") && <kbd>{keyHint("pane.add")}</kbd>}
      </button>
      {open && <AddPaneDrawer client={client} tab={tab} />}
    </div>
  );
}

function hint(p: Preset): string {
  switch (p) {
    case "columns":
      return "Columns: every pane side by side";
    case "grid":
      return "Grid: rows of equal panes";
    case "main_stack":
      return "Main + stack: the active pane large, the rest stacked beside it";
  }
}

/** What each preset looks like, drawn small. */
function Thumb({ preset }: { preset: Preset }) {
  const r = (x: number, y: number, w: number, h: number) => <rect x={x} y={y} width={w} height={h} rx="1" />;
  return (
    <svg class="pt-thumb" width="18" height="13" viewBox="0 0 18 13" aria-hidden="true">
      {preset === "columns"
        ? [r(0.5, 0.5, 5, 12), r(6.5, 0.5, 5, 12), r(12.5, 0.5, 5, 12)]
        : preset === "grid"
          ? [r(0.5, 0.5, 8, 5.5), r(9.5, 0.5, 8, 5.5), r(0.5, 7, 8, 5.5), r(9.5, 7, 8, 5.5)]
          : [r(0.5, 0.5, 10, 12), r(11.5, 0.5, 6, 3.5), r(11.5, 4.75, 6, 3.5), r(11.5, 9, 6, 3.5)]}
    </svg>
  );
}

const FocusIcon = ({ on }: { on: boolean }) => (
  <svg class="icon" width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {on ? <path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" /> : <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />}
  </svg>
);
