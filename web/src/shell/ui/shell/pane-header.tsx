// The identity and controls above each desktop pane.

import type { Client } from "../../client";
import type { PaneId, PaneInfo, Rect, TabView } from "../../../shared/proto";
import type { Cell } from "../cells";
import { paneItems } from "../commands";
import { startDrag } from "../drag";
import { keyHint } from "../keys";
import { openMenu } from "../../../shared/ui/menu";
import { openFresh } from "../open-fresh";
import { drop } from "../panes/drop";
import { len } from "../panes/geometry";
import { layoutRefusal, ordinal, toggleFocus } from "../panes/layout";
import { BranchIcon, MoreIcon } from "./icons";
import { HintChip } from "../hints";

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
    case "team_chat":
      return "chat";
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

/** Blocks with a program behind them, whose header says how it is; the
 * rest (a page, a file, a PR) have no run state, so no "idle" (UI audit
 * #177). Needing you, or done, still shows on any. */
const RUNS = new Set<string | undefined>([undefined, "terminal", "remote", "agent", "chat"]);

/** The glyph a pane's header draws (UI audit #177: one per block type): a
 * forge block is a pull request, an issue, or a project's own issue. */
export function paneGlyph(client: Client, id: PaneId, info: PaneInfo | undefined): { glyph: string; word: string } {
  if (info?.type === "forge") {
    const s = client.blocks.get(id)?.state as { kind?: string; native?: boolean } | null | undefined;
    if (s?.native) return { glyph: "native-issue", word: "project issue" };
    return s?.kind === "issue" ? { glyph: "issue", word: "issue" } : { glyph: "pr", word: "pull request" };
  }
  return { glyph: info?.type ?? "terminal", word: paneType(info) };
}

/** Where a pane runs when that isn't here: a remote pane's host, a VM. */
function hostOf(client: Client, id: PaneId, info: PaneInfo | undefined): string | null {
  if (info?.type === "remote") return (client.blocks.get(id)?.state as { host?: string } | null)?.host ?? "remote";
  const m = client.machine(id);
  return m ? (m.name ?? m.sprite) : null;
}

/** What names a pane, above it, and its handle (M110): its number, kind,
 * what it's running (its title, without the spinner a program puts in
 * front), status, host, repo and branch, which pane has the keys, a click
 * to give it the keys, a drag to move it, Focus and its menu. It's one
 * reserved pixel row above the pane, outside its terminal cells. */
export function PaneHeader({
  client,
  tab,
  id,
  rect,
  cell,
  active,
  height = cell.height,
  sizedElsewhere,
}: {
  client: Client;
  tab: TabView;
  id: PaneId;
  rect: Rect;
  cell: Cell;
  active: boolean;
  height?: number;
  /** Another window sizes the tab: a chip here says so, and its button
   * takes the size for this window (UI audit #171). */
  sizedElsewhere?: () => void;
}) {
  const info = client.info(id);
  const title = client.title(id) || info?.command || `pane %${id}`;
  const cwd = client.cwd(id);
  const where = info?.project?.name ?? (cwd ? cwd.split("/").filter(Boolean).pop() ?? "/" : null);
  const style = { left: len(rect.x * cell.width), top: len(rect.y * cell.height - height), width: len(rect.cols * cell.width), height: len(height) };

  const n = ordinal(tab, id);
  const { glyph, word: type } = paneGlyph(client, id, info);
  const said = paneStatus(info);
  const status = RUNS.has(info?.type) || said.cls === "needs_input" || said.cls === "done" ? said : { cls: "idle", word: "" };
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
          {String(n).padStart(2, "0")}
        </span>
      )}
      <span class="pane-header-type" role="img" aria-label={type} title={type}><PaneTypeIcon type={glyph} /></span>
      <span class="pane-header-title">{title}</span>
      {status.word && client.connected && (
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
      <HintChip client={client} pane={id} />
      {sizedElsewhere && (
        <span class="pane-header-sized" data-sized-elsewhere title="Another window set this tab's size">
          <span class="pane-header-sized-what">Sized by another window</span>
          <button aria-label="Use this window's size" title="Use this window's size" onClick={sizedElsewhere}>
            Use this one
          </button>
        </span>
      )}
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

/** Type remains named for assistive technology and in the hover label.
 * Each block type draws its own (UI audit #177). */
const GLYPHS: Record<string, string> = {
  terminal: "M4 6l6 6-6 6M12 18h8",
  remote: "M3 4h18v12H3zM7 8l3 2-3 2M12 12h4M8 20h8M12 16v4",
  agent: "M7 8h10v11H7zM12 8V4M9 12h1M14 12h1M10 16h4",
  chat: "M4 5h16v11H9l-5 4z",
  team_chat: "M3 4h12v9H7l-4 3zM18 8h3v11l-3-3h-7v-3",
  editor: "M8 7l-5 5 5 5M16 7l5 5-5 5M14 4l-4 16",
  file: "M6 3h8l4 4v14H6zM14 3v5h4M10.5 12l-2 2.5 2 2.5M13.5 12l2 2.5-2 2.5",
  doc: "M5 3h14v18H5zM8 7h8M8 11h8M8 15h5",
  diff: "M6 3v18M18 3v18M3 8h6M15 16h6M18 13v6",
  browser: "M3 5h18v14H3zM3 9h18M6 7h1M9 7h1",
  pr: "M6 3a2 2 0 1 0 0 4a2 2 0 1 0 0-4M6 7v10M6 17a2 2 0 1 0 0 4a2 2 0 1 0 0-4M18 17a2 2 0 1 0 0 4a2 2 0 1 0 0-4M18 17V9a3 3 0 0 0-3-3h-4M13 4l-2 2 2 2",
  issue: "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18M12 10.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 1 0 0-3",
  "native-issue": "M4 4h16v16H4zM12 10.5a1.5 1.5 0 1 0 0 3a1.5 1.5 0 1 0 0-3",
  workspace: "M12 3l9 5-9 5-9-5zM3 13l9 5 9-5",
  app: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM16.5 13v7M13 16.5h7",
  invite: "M9 5a3 3 0 1 0 0 6a3 3 0 1 0 0-6M3 20c0-3.3 2.7-5.5 6-5.5s6 2.2 6 5.5M19 8v6M16 11h6",
  agents: "M3 10h8v8H3zM13 10h8v8h-8zM7 10V6M17 10V6M6 14h.5M8.5 14h.5M16 14h.5M18.5 14h.5",
};

function PaneTypeIcon({ type }: { type: string }) {
  const path = GLYPHS[type] ?? GLYPHS.terminal;
  return <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" data-pane-glyph={type}><path d={path} /></svg>;
}
