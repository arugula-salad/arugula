// The row above a pane, when a tab has more than one.

import type { Client } from "../../client";
import type { PaneId, PaneInfo, Rect, TabView } from "../../proto";
import type { Cell } from "../cells";
import { paneItems } from "../commands";
import { startDrag } from "../drag";
import { keyHint } from "../keys";
import { openMenu } from "../menu";
import { openFresh } from "../open-fresh";
import { drop } from "../panes/drop";
import { len } from "../panes/geometry";
import { layoutRefusal, ordinal, toggleFocus } from "../panes/layout";
import { BranchIcon, MoreIcon } from "./icons";

/** What kind of pane it is, in a word. */
export function paneType(info: PaneInfo | undefined): string {
  switch (info?.type) {
    case undefined:
      return "pane";
    case "terminal":
      return info.kind && info.kind !== "shell" ? info.kind : "shell";
    case "diff":
      return "changes";
    case "browser":
      return "web";
    case "remote":
      return "remote shell";
    default:
      return info?.type ?? "pane";
  }
}

/** How the pane is, from what the daemon says: its attention, else
 * whether its program still runs. */
export function paneStatus(info: PaneInfo | undefined): { cls: string; word: string } {
  if (!info) return { cls: "idle", word: "" };
  if (info.attention === "needs_input") return { cls: "needs_input", word: "needs you" };
  if (info.attention === "done") {
    const k = info.reason?.kind;
    return { cls: "done", word: k === "failed" ? "failed" : k === "exited" ? "exited" : "done" };
  }
  if (info.running === false) return { cls: "stopped", word: "stopped" };
  if (info.attention === "working") return { cls: "working", word: "working" };
  return { cls: "idle", word: "idle" };
}

/** Where a pane runs when that isn't here: a remote pane's host, a VM. */
function hostOf(client: Client, id: PaneId, info: PaneInfo | undefined): string | null {
  if (info?.type === "remote") return (client.blocks.get(id)?.state as { host?: string } | null)?.host ?? "remote";
  const m = client.machine(id);
  return m ? (m.name ?? m.sprite) : null;
}

export /** What names a pane, above it: what it's running (its title, without the
 * spinner a program puts in front), its project or directory, the
 * machine when it isn't the tab's, and which pane has the keys. A label
 * only, in the classic bar (the divider under it stays draggable); with
 * `live` (the new workspace, M110) it's the pane's handle: its number,
 * kind, status, host, repo and branch, a click to give it the keys, a
 * drag to move it, Focus and its menu. It stays one cell row either way. */
function PaneHeader({
  client,
  tab,
  id,
  rect,
  cell,
  active,
  live,
}: {
  client: Client;
  tab: TabView;
  id: PaneId;
  rect: Rect;
  cell: Cell;
  active: boolean;
  live?: boolean;
}) {
  const info = client.info(id);
  const title = client.title(id) || info?.command || `pane %${id}`;
  const cwd = client.cwd(id);
  const where = info?.project?.name ?? (cwd ? cwd.split("/").filter(Boolean).pop() ?? "/" : null);
  const style = { left: len(rect.x * cell.width), top: len((rect.y - 1) * cell.height), width: len(rect.cols * cell.width), height: len(cell.height) };
  if (!live) {
    const m = client.machine(id);
    return (
      <div
        class={active ? "pane-header active" : "pane-header"}
        data-pane-header={id}
        aria-current={active ? "true" : undefined}
        title={active ? "This pane has the keyboard" : undefined}
        style={style}
      >
        <span class="pane-header-title">{title}</span>
        {where && title !== where ? <span class="pane-header-where">{where}</span> : null}
        {m ? <span class="pane-header-where">{m.name ?? m.sprite}</span> : null}
      </div>
    );
  }

  const n = ordinal(tab, id);
  const type = paneType(info);
  const status = paneStatus(info);
  const host = hostOf(client, id, info);
  const branch = info?.branch ?? null;
  const focused = tab.focus === id;
  const why = layoutRefusal(client, tab);
  const jump = n >= 1 && n <= 9 ? keyHint("pane.jump")?.replace("1–9", String(n)) : undefined;
  const give = () => {
    client.setActive(id);
    client.viewOf(id)?.focus();
  };
  const more = (e: MouseEvent) => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const at = { clientX: r.left, clientY: r.bottom, preventDefault() {} };
    give();
    const items = () => paneItems(client, id, false, null);
    if (info?.type === "remote") openMenu(at, items());
    else openFresh(client, at, items);
  };
  return (
    <div
      class={`pane-header live${active ? " active" : ""}`}
      data-pane-header={id}
      aria-current={active ? "true" : undefined}
      style={style}
      title={`${title}: click to type here, drag to move`}
      onPointerDown={(e) => {
        if ((e.target as HTMLElement).closest("button")) return;
        e.preventDefault();
        startDrag(e, { kind: "pane", pane: id }, title, { onDrop: (what, target) => drop(client, what, target), onClick: give });
      }}
      onDblClick={(e) => !(e.target as HTMLElement).closest("button") && !why && toggleFocus(client, tab, id)}
      onContextMenu={(e) => {
        e.preventDefault();
        more(e);
      }}
    >
      {n > 0 && (
        <span class="pane-header-n" title={jump ? `Pane ${n} (${jump})` : `Pane ${n}`} data-pane-ordinal={n}>
          {n}
        </span>
      )}
      <span class="pane-header-type">{type}</span>
      <span class="pane-header-title">{title}</span>
      {status.word && (
        <span class={`pane-header-status ${status.cls}`} data-pane-status={status.cls}>
          <span class={`ws-dot ${status.cls}`} aria-hidden="true" />
          {status.word}
        </span>
      )}
      {host && <span class="pane-header-host" title={`Runs on ${host}`}>{host}</span>}
      {where && where !== title && <span class="pane-header-where" title={cwd ?? where}>{where}</span>}
      {branch && (
        <span class="pane-header-branch" title={`Branch ${branch}`}>
          <BranchIcon size={11} />
          {branch}
        </span>
      )}
      <span class="pane-header-fill" />
      <button
        class={`pane-header-button${focused ? " on" : ""}`}
        data-pane-expand={id}
        aria-pressed={focused}
        aria-label={focused ? "Restore layout" : "Focus this pane"}
        title={why ?? `${focused ? "Restore layout" : "Focus this pane"} (${keyHint("pane.focus")})`}
        disabled={!!why}
        onClick={() => toggleFocus(client, tab, id)}
      >
        <ExpandIcon on={focused} />
      </button>
      <button class="pane-header-button" data-pane-more={id} aria-label="Pane menu" title="Pane menu" onClick={more}>
        <MoreIcon size={13} />
      </button>
    </div>
  );
}

/** Two corners out (focus), or in (restore). */
const ExpandIcon = ({ on }: { on: boolean }) => (
  <svg class="icon" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {on ? <path d="M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" /> : <path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" />}
  </svg>
);
