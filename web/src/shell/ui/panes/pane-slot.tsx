// One pane in the grid: the block's view moved in, with what it carries on
// top (badges, an answer card, the grip to drag it by) and its menu.

import { useEffect, useLayoutEffect, useRef } from "preact/hooks";
import type { Client } from "../../client";
import type { PaneId, Rect } from "../../../shared/proto";
import { useWorkspaceDir } from "../../blocks";
import type { Cell } from "../cells";
import { paneItems } from "../commands";
import { startDrag } from "../drag";
import { openMenu } from "../../../shared/ui/menu";
import { openFresh } from "../open-fresh";
import { PaneMarks } from "../people";
import { HooksChip } from "../agents-setup";
import { drawsItsOwn, TermAnswered, TermAsk, TermDiff } from "../term-ask";
import { ThreadBadge } from "../threads";
import { HintPaneModal } from "../hints";
import { drop } from "./drop";
import { px } from "./geometry";

/** Where hidden panes' terminals live: off the page but still alive. */
const parking = document.createElement("div");
parking.id = "parking";
document.body.appendChild(parking);

/** A text box of the page's own, not xterm's hidden one (a terminal's
 * right-click lands there). */
function editable(t: EventTarget | null): boolean {
  return t instanceof HTMLElement && t.matches("textarea, input") && !t.closest(".xterm");
}

export function PaneSlot({
  client,
  id,
  rect,
  cell,
  active,
  phone,
}: {
  client: Client;
  id: PaneId;
  rect: Rect;
  cell: Cell;
  active: boolean;
  phone: boolean;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // A terminal's entry (for terminal-only features), and the view of any
  // block type.
  const entry = client.panes.get(id);
  const view = client.viewOf(id);

  // Move the block's view in, rather than creating one: moving a block
  // around the layout never restarts or redraws it from scratch.
  useLayoutEffect(() => {
    const slot = ref.current;
    if (!view || !slot) return;
    slot.appendChild(view.host);
    view.setVisible(true);
    return () => {
      if (view.host.parentElement === slot) {
        parking.appendChild(view.host);
        view.setVisible(false);
      }
    };
  }, [view]);

  const info = client.info(id);
  // M34: a chant workspace where it runs, to open as one.
  const workspaceDir = client.cwd(id);
  const isWorkspace = useWorkspaceDir(client, id, info?.type === "terminal" ? workspaceDir : null);

  // Right-clicking a command's mark.
  useEffect(() => {
    entry?.view.onMarkMenu((mark, e) => {
      const view = entry.view;
      openMenu(e, [
        { header: mark.text || "command" },
        { label: "Select output", run: () => view.selectOutput(mark) },
        { label: "Copy output", run: () => void navigator.clipboard?.writeText(view.outputText(mark)) },
        { label: "Copy command", disabled: !mark.text, run: () => void navigator.clipboard?.writeText(mark.text) },
        "separator",
        {
          label: "Run again",
          disabled: !mark.text || client.info(id)?.current !== null,
          run: () => client.input(id, new TextEncoder().encode(`${mark.text}\r`)),
        },
      ]);
    });
  }, [entry, client, id]);

  // A right-click is our menu's, also over a program that tracks the mouse
  // (most agents do, and ignore the button). Shift sends it on to the
  // program instead, as a plain right-click: xterm.js would report the
  // Shift too, and no program binds that.
  const rightDown = (e: MouseEvent) => {
    if (e.button !== 2 || !e.isTrusted || !entry?.view.mouseTracking) return;
    e.stopPropagation();
    if (!e.shiftKey) return;
    const plain = (type: string, from: MouseEvent) =>
      new MouseEvent(type, {
        bubbles: true,
        cancelable: true,
        view: window,
        button: from.button,
        buttons: from.buttons,
        clientX: from.clientX,
        clientY: from.clientY,
      });
    e.target?.dispatchEvent(plain("mousedown", e));
    const up = (u: MouseEvent) => {
      if (u.button !== 2 || !u.isTrusted) return;
      window.removeEventListener("mouseup", up, true);
      u.stopPropagation();
      (u.target ?? document).dispatchEvent(plain("mouseup", u));
    };
    window.addEventListener("mouseup", up, true);
  };

  const menu = (e: MouseEvent) => {
    // Sent on to the program (above): no menu, ours or the browser's.
    if (entry?.view.mouseTracking && e.shiftKey) return e.preventDefault();
    // A text box's own menu (an agent's composer): on a phone, its Paste
    // is the one that can paste an image (Chrome refuses ours).
    if (editable(e.target)) return;
    const items = () => paneItems(client, id, phone, isWorkspace ? workspaceDir : null);
    // A remote pane's menu doesn't depend on this daemon's features.
    if (info?.type === "remote") openMenu(e, items());
    else openFresh(client, e, items);
  };

  // A terminal's shell waiting for Enter; a stopped agent block isn't
  // running either, but has its own Resume (UI audit #154).
  const waiting = client.info(id)?.running === false && (info?.type ?? "terminal") === "terminal";
  return (
    <div
      ref={ref}
      class={active ? "pane active" : "pane"}
      data-pane={id}
      style={px(rect, cell)}
      onPointerDownCapture={() => client.setActive(id)}
      onMouseDownCapture={rightDown}
      onContextMenu={menu}
    >
      <HostBadge client={client} id={id} />
      <StartedByBadge client={client} id={id} />
      <PaneMarks client={client} pane={id} />
      {info?.type !== "remote" && <ThreadBadge client={client} pane={id} />}
      {!active && (info?.attention === "needs_input" || info?.attention === "done") && (
        <div class={`pane-badge ${info.attention}`} title={info.reason?.headline}>
          {info.reason?.kind === "failed" ? "failed" : info.reason?.kind === "exited" ? "exited" : info.attention === "done" ? "done" : "needs you"}
        </div>
      )}
      {info?.diff ? <TermDiff client={client} id={id} diff={info.diff} /> : info?.ask && !drawsItsOwn(info.ask) && <TermAsk client={client} id={id} ask={info.ask} />}
      {!info?.ask && !info?.diff && info?.answered && info.type === "terminal" && <TermAnswered client={client} id={id} answered={info.answered} />}
      {info?.unhooked && !info.ask && <HooksChip client={client} id={id} />}
      {waiting && (
        <button
          class="start-pane"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={() => client.input(id, new TextEncoder().encode("\r"))}
        >
          {client.info(id)?.policy.kind === "rerun" ? "Re-run" : "Start shell"}
        </button>
      )}
      {/* A big tip over this pane (shell/hints): a computer's alone. */}
      {!phone && <HintPaneModal client={client} pane={id} />}
      {!phone && (
        <div
          class="grip"
          title="Drag to move this pane"
          onPointerDown={(e) => {
            e.stopPropagation();
            startDrag(e, { kind: "pane", pane: id }, client.title(id) || `pane %${id}`, {
              onDrop: (what, target) => drop(client, what, target),
            });
          }}
        >
          ⠿
        </div>
      )}
    </div>
  );
}

/** What started a pane, when it was an MCP client (M16): an agent outside, or an agent block's. */
function StartedByBadge({ client, id }: { client: Client; id: PaneId }) {
  const s = client.info(id)?.started_by;
  if (!s) return null;
  const via = s.block !== undefined ? `, from agent block %${s.block}` : "";
  return (
    <div class="started-by" data-started-by={s.by} title={`Started through MCP by ${s.by}${via}. What it typed is in history as theirs.`}>
      started by {s.by}
    </div>
  );
}

/**
 * Where a pane runs, when that isn't where its tab says: a VM pane in an
 * ordinary tab, or a local pane in a VM tab.
 */
function HostBadge({ client, id }: { client: Client; id: PaneId }) {
  const m = client.machine(id);
  const tab = client.tabOfPane(id);
  const tabMachine = tab ? client.tabMachine(tab.id) : undefined;
  if (tabMachine && !m) {
    return (
      <div class="host-badge local" title="Runs on this host, not the tab's machine">
        local
      </div>
    );
  }
  if (!m || m.id === tabMachine?.id) return null;
  const state = m.state === "running" ? "" : ` · ${m.state === "gone" ? "gone" : "starting"}`;
  if (m.borrowed) {
    return (
      <div
        class={`host-badge borrowed ${m.state}`}
        title={`A shell on ${m.sprite} (${m.provider}) with no daemon there: disposable. Its output is kept here while it's attached; the sandbox stays when this pane closes. Make it resident (Sandboxes…) for history that survives.`}
      >
        {m.sprite} · shell{state}
      </div>
    );
  }
  return (
    <div
      class={`host-badge ${m.state}`}
      title={`${m.name ? `${m.name}: ` : ""}${m.sprite} (${m.provider}${m.image ? `, ${m.image}` : ""}); deleted when this pane closes`}
    >
      VM{state}
    </div>
  );
}
