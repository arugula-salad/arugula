// A tab's panes, laid out at the cells the daemon computed.

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../../client";
import type { PaneId, TabId, TabView } from "../../proto";
import type { Cell } from "../cells";
import { PaneHeader } from "../shell/pane-header";
import { Divider } from "./divider";
import { DropOverlay } from "./drop-overlay";
import { len } from "./geometry";
import { PaneSlot } from "./pane-slot";

/** One tab's panes, drawn at the cell rectangles the daemon computed. The
 * whole grid is scaled down when another window's size owns the tab. On a
 * computer the headers and gutters are handles (M110). */
export function TabArea({ client, tab, cell, phone }: { client: Client; tab: TabView; cell: Cell; phone: boolean }) {
  const ref = useRef<HTMLDivElement>(null);
  const [area, setArea] = useState<{ w: number; h: number } | null>(null);

  useLayoutEffect(() => {
    const el = ref.current!;
    // Inside its padding: the grid keeps clear of the window's edges (#164).
    const measure = () => {
      const s = getComputedStyle(el);
      const padX = parseFloat(s.paddingLeft) + parseFloat(s.paddingRight);
      const padY = parseFloat(s.paddingTop) + parseFloat(s.paddingBottom);
      setArea({ w: el.clientWidth - padX, h: el.clientHeight - padY });
    };
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    measure();
    return () => ro.disconnect();
  }, []);

  // With more than one pane, each gets a header, one cell row above it:
  // a pane below another has its header in the divider row between them,
  // and the top row's take a row above the grid (like tmux's
  // pane-border-status). A phone shows one pane at a time: none.
  const headers = !phone && tab.layout.panes.length > 1;
  const cols = area ? Math.max(2, Math.floor(area.w / cell.width)) : 0;
  const rows = area ? Math.max(1, Math.floor((area.h - (headers ? cell.height : 0)) / cell.height)) : 0;
  const active = client.active(tab.id) ?? null;
  const zoom = phone ? active : null;

  // A tab focused on one pane (M109, anyone's doing) has the keys there.
  useEffect(() => {
    if (tab.focus !== null && client.active(tab.id) !== tab.focus) client.setActive(tab.focus);
  }, [client, tab.id, tab.focus]);

  // Tell the daemon what we show. Showing a tab (or another pane, on a
  // phone) claims its size; resizing the window only updates a size we own.
  const size = useRef({ cols, rows, zoom });
  size.current = { cols, rows, zoom };
  const shown = useRef<{ tab: TabId; zoom: PaneId | null } | null>(null);
  useEffect(() => {
    client.claim = (t, typed) => {
      const s = size.current;
      if (s.cols) client.view(t, s.cols, s.rows, s.zoom, true, typed);
    };
  }, [client]);
  useEffect(() => {
    if (!cols) return;
    const prev = shown.current;
    const claim = !prev || prev.tab !== tab.id || prev.zoom !== zoom;
    shown.current = { tab: tab.id, zoom };
    client.view(tab.id, cols, rows, zoom, claim);
  }, [client, tab.id, cols, rows, zoom]);

  // Keyboard focus follows the active pane (not on phones: that would pop
  // up the keyboard on every switch).
  // The active pane when the effect runs, not when it rendered: a pane
  // picked from the keyboard (Ctrl+Shift+N) in the frame between would
  // otherwise lose the keys to the one before it.
  useEffect(() => {
    const now = client.active(tab.id) ?? null;
    if (!phone && now !== null) client.viewOf(now)?.focus();
  }, [client, tab.id, active, phone]);
  // ...and comes back to it after menus and buttons, unless something else
  // (a rename box) is using the keyboard.
  const rev = client.state?.rev;
  useEffect(() => {
    const el = document.activeElement;
    const idle = !el || el === document.body || el.closest(".menu, .bar button, .tab, .pane-toolbar button, .pane-header button");
    const now = client.active(tab.id) ?? null;
    if (!phone && now !== null && idle) client.viewOf(now)?.focus();
  }, [client, rev, active, phone]);

  const gridW = tab.cols * cell.width;
  const gridH = tab.rows * cell.height;
  const scale = area ? Math.min(1, area.w / gridW, area.h / gridH) : 1;
  const elsewhere = tab.owner !== null && tab.owner !== client.clientId;

  return (
    <div class={scale < 1 ? "tab-area scaled" : "tab-area"} ref={ref}>
      <div
        class={headers ? "grid with-headers" : "grid"}
        style={{
          width: len(gridW),
          height: len(gridH),
          marginTop: headers ? len(cell.height) : undefined,
          transform: scale < 1 ? `scale(${scale})` : undefined,
        }}
      >
        {tab.layout.panes.map(([id, r]) => (
          <PaneSlot key={id} client={client} id={id} rect={r} cell={cell} active={id === active} phone={phone} />
        ))}
        {headers &&
          tab.layout.panes.map(([id, r]) => (
            <PaneHeader key={`h${id}`} client={client} tab={tab} id={id} rect={r} cell={cell} active={id === active} />
          ))}
        {!phone &&
          tab.layout.splits.map((s) =>
            s.extents.slice(0, -1).map((_, i) => (
              <Divider key={`${s.id}-${i}`} client={client} split={s} index={i} cell={cell} scale={scale} />
            )),
          )}
        <DropOverlay client={client} tab={tab} cell={cell} />
      </div>
      {elsewhere && (
        <div class="sized-elsewhere" data-sized-elsewhere>
          Another window set this tab's size.
          <button onClick={() => client.claim(tab.id)}>Use this window's size</button>
        </div>
      )}
    </div>
  );
}
