// The identity and controls above each desktop pane.

import type { Client } from "../../client";
import type { PaneId, PaneInfo, Rect, TabView } from "../../../shared/proto";
import type { Cell } from "../cells";
import { paneItems } from "../commands";
import { startDrag } from "../drag";
import { keyHint } from "../keys";
import { openFresh } from "../open-fresh";
import { drop } from "../panes/drop";
import { len } from "../panes/geometry";
import { layoutRefusal, ordinal, toggleFocus } from "../panes/layout";
import { BranchIcon, FocusIcon, MoreIcon, PaneTypeIcon } from "../../../shared/ui/icons";
import { HintChip } from "../hints";
import { ContextStrip } from "../../blocks/agent-context";
import { Bar, BarTitle, BarWhere } from "../../../shared/ui/bar";
import { PaneCardChip } from "../pane-card";
import { chipRoom } from "../pane-card-text";

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
const RUNS = new Set<string | undefined>([undefined, "terminal", "agent", "chat"]);

/** The glyph a pane's header draws (UI audit #177: one per block type): a
 * forge block is a pull request, an issue, or a project's own issue. */
export function paneGlyph(client: Client, id: PaneId, info: PaneInfo | undefined): { glyph: string; word: string } {
  if (info?.type === "forge") {
    const s = client.blocks.get(id)?.state as { kind?: string; native?: boolean } | null | undefined;
    if (s?.native) return { glyph: "native-issue", word: "project issue" };
    return s?.kind === "issue" ? { glyph: "issue", word: "issue" } : { glyph: "pr", word: "pull request" };
  }
  // #1734: an nvim editor block, a terminal typed as an editor.
  if (info?.app === "nvim") return { glyph: "nvim", word: "nvim editor" };
  return { glyph: info?.type ?? "terminal", word: paneType(info) };
}

/** Where a pane runs when that isn't here: a VM. */
function hostOf(client: Client, id: PaneId): string | null {
  const m = client.machine(id);
  return m ? (m.name ?? m.sprite) : null;
}

/** What names a pane, above it, and its handle (M110): its number, kind,
 * what it's running (its title, without the spinner a program puts in
 * front), status, host, repo, its card and branch, which pane has the keys, a click
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
  const host = hostOf(client, id);
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
    openFresh(client, at, items);
  };
  return (
    <Bar
      class={`pane-header live${active ? " active" : ""}`}
      data-pane-header={id}
      aria-current={active ? "true" : undefined}
      style={style}
      tip={`${title}: click to type here, drag to move`}
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
      lead={
        <>
          {n > 0 && (
            <span class="pane-header-n" title={jump ? `Pane ${n} (${jump})` : `Pane ${n}`} data-pane-ordinal={n}>
              {String(n).padStart(2, "0")}
            </span>
          )}
          <span class="pane-header-type" role="img" aria-label={type} title={type}><PaneTypeIcon type={glyph} /></span>
        </>
      }
      title={<BarTitle class="pane-header-title">{title}</BarTitle>}
      status={
        <>
          {status.word && client.connected && (
            <span class={`pane-header-status ${status.cls}`} data-pane-status={status.cls}>
              <span class={`ws-dot ${status.cls}`} aria-hidden="true" />
              {status.word}
            </span>
          )}
          {host && <span class="pane-header-host" title={`Runs on ${host}`}>{host}</span>}
        </>
      }
      where={where && where !== title ? <BarWhere class="pane-header-where" title={cwd ?? where}>{where}</BarWhere> : undefined}
      actions={
        <>
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
            class={`pane-header-button bar-btn sq${focused ? " on" : ""}`}
            data-pane-expand={id}
            aria-pressed={focused}
            aria-label={focused ? "Restore layout" : "Focus this pane"}
            title={why ?? `${focused ? "Restore layout" : "Focus this pane"} (${keyHint("pane.focus")})`}
            disabled={!!why}
            onClick={() => toggleFocus(client, tab, id)}
          >
            <FocusIcon on={focused} />
          </button>
          <button class="pane-header-button bar-btn sq" data-pane-more={id} aria-label="Pane menu" title="Pane menu" onClick={more}>
            <MoreIcon size="xs" />
          </button>
        </>
      }
    >
      <PaneCardChip client={client} pane={id} room={chipRoom(rect.cols * cell.width)} />
      {branch && (
        <span class="pane-header-branch" title={`Branch ${branch}`}>
          <BranchIcon size="xs" />
          {branch}
        </span>
      )}
      {/* #450: Claude Code running here, as it says from inside (its mod). */}
      {info?.live && info.type !== "agent" && <ContextStrip c={info.live} slim />}
    </Bar>
  );
}

