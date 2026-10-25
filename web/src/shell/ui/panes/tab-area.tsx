// A tab's panes, laid out at the cells the daemon computed.

import { useEffect, useLayoutEffect, useRef, useState } from "preact/hooks";
import type { Client } from "../../client";
import type { PaneId, TabId, TabView } from "../../../shared/proto";
import type { Cell } from "../cells";
import { PaneHeader } from "../shell/pane-header";
import { Divider } from "./divider";
import { DropOverlay } from "./drop-overlay";
import { len, px } from "./geometry";
import { paneChrome, PIXELS } from "./chrome-geometry";
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

  // Reserve the split tree's chrome budget before claiming terminal rows.
  // Phones keep their existing single-pane, cell-only layout.
  const headers = !phone && tab.layout.panes.length > 0;
  const chrome = headers ? paneChrome(tab.layout, tab.cols, tab.rows, cell) : null;
  const cols = area ? Math.max(2, Math.floor(area.w / cell.width)) : 0;
  const rows = area ? Math.max(1, Math.floor((area.h - (chrome?.overhead ?? 0)) / cell.height)) : 0;
  const active = client.active(tab.id) ?? null;
  const zoom = phone ? active : null;

  // An effect of a render whose tab isn't shown any more (#1049): Preact
  // runs a render's effects as late as the next render, and by then a tab
  // just made can be the shown one. Focusing a pane selects its tab, so a
  // stale effect would select the old tab, whose effect selects the new
  // one, and the two swap for good, the page stuck redrawing.
  const stale = () => client.tab !== tab.id;

  // A tab focused on one pane (M109, anyone's doing) has the keys there.
  useEffect(() => {
    if (stale()) return;
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
  // Not from a dialog that has them, though: this runs after the next
  // paint, and a palette opened in between (Enter on a tab, then
  // Ctrl+Shift+P) lost what was typed into it to the terminal's shell.
  useEffect(() => {
    if (stale() || inDialog()) return;
    const now = client.active(tab.id) ?? null;
    if (!phone && now !== null) client.viewOf(now)?.focus();
  }, [client, tab.id, active, phone]);
  // ...and comes back to it after menus and buttons, unless something else
  // (a rename box) is using the keyboard.
  const rev = client.state?.rev;
  useEffect(() => {
    if (stale()) return;
    const el = document.activeElement;
    // Not from where the keyboard took it (M113): someone moving through
    // the chrome with Tab and the arrows keeps their place.
    const idle =
      !el ||
      el === document.body ||
      (el.closest(".menu, .bar button, .tab, .pane-toolbar button, .pane-header button") && !el.matches(":focus-visible"));
    const now = client.active(tab.id) ?? null;
    if (!phone && now !== null && idle) client.viewOf(now)?.focus();
  }, [client, rev, active, phone]);

  const gridW = tab.cols * cell.width;
  const gridH = chrome?.height ?? tab.rows * cell.height;
  const drawingCell = chrome ? PIXELS : cell;
  const drawingTab = chrome ? { ...tab, layout: { ...tab.layout, panes: tab.layout.panes.map(([id, r]) => [id, chrome.panes.get(id) ?? r] as [PaneId, typeof r]) } } : tab;
  const scale = area ? Math.min(1, area.w / gridW, area.h / gridH) : 1;
  // Not while the link is down, nor to a viewer: neither has a say in its
  // size (viewers see it letterboxed; the daemon takes editors' sizes).
  const viewer = client.role(client.sessionOfTab(tab.id) ?? null) === "viewer";
  const presenting = tab.presenter !== undefined;
  const mine = presenting && tab.presenter === client.clientId;
  // Someone else's window sets the size while they present (#1472), so the
  // "Use this window's size" bar gives way to theirs.
  const elsewhere = !presenting && client.connected && !viewer && tab.owner !== null && tab.owner !== client.clientId;
  const presenter = client.state?.presence?.find((p) => p.client === tab.presenter)?.name ?? "Someone";

  return (
    <div class={scale < 1 ? "tab-area scaled" : "tab-area"} ref={ref}>
      <div
        class={headers ? "grid with-headers pane-chrome" : "grid"}
        style={{
          width: len(gridW),
          height: len(gridH),
          transform: scale < 1 ? `scale(${scale})` : undefined,
        }}
      >
        {drawingTab.layout.panes.map(([id, r]) => (
          <PaneSlot key={id} client={client} id={id} rect={r} cell={drawingCell} active={id === active} phone={phone} />
        ))}
        {headers &&
          drawingTab.layout.panes.map(([id, r]) => (
            <PaneHeader key={`h${id}`} client={client} tab={tab} id={id} rect={r} cell={drawingCell} height={chrome?.header} active={id === active} />
          ))}
        {chrome && Array.from(chrome.frames, ([id, r]) => (
          <div key={`f${id}`} class={`pane-frame${id === active ? " active" : ""}`} style={px(r, PIXELS)} aria-hidden="true" />
        ))}
        {!phone &&
          tab.layout.splits.map((s) =>
            s.extents.slice(0, -1).map((_, i) => (
              <Divider key={`${s.id}-${i}`} client={client} split={s} index={i} cell={cell} scale={scale} projected={chrome?.dividers.get(`${s.id}-${i}`)} resizeUnit={chrome?.resizeUnits.get(`${s.id}-${i}`)} />
            )),
          )}
        <DropOverlay client={client} tab={drawingTab} cell={drawingCell} />
      </div>
      {presenting && client.connected && (
        <div class="sized-elsewhere" data-presenter-bar data-presenting={mine ? "me" : "other"}>
          {mine ? "Presenting" : `${presenter} is presenting`}
          {mine ? (
            <button onClick={() => client.present(tab.id, false)}>Stop</button>
          ) : (
            !viewer && <button onClick={() => client.present(tab.id, true)}>Take over</button>
          )}
        </div>
      )}
      {elsewhere && (
        <div class="sized-elsewhere" data-sized-elsewhere>
          Another window set this tab's size.
          <button onClick={() => client.claim(tab.id)}>Use this window's size</button>
        </div>
      )}
    </div>
  );
}

/** The keys are in a dialog: the palette, a prompt, a picker. */
const inDialog = () => !!document.activeElement?.closest('[role="dialog"], [role="alertdialog"]');
