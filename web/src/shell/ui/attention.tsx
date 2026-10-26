import { paneIds, type Client } from "../client";
import type { Attention, PaneId, Reason, TabView } from "../../shared/proto";
import { openMenu, type MenuItem } from "../../shared/ui/menu";

/** The most urgent attention among a tab's panes, and why. */
export function tabAttention(client: Client, tab: TabView): { state: Attention; reason?: Reason | null; pane?: PaneId } {
  const infos = paneIds(tab).map((p) => client.info(p));
  const pick = (s: Attention) => infos.find((i) => i?.attention === s);
  const top = pick("needs_input") ?? pick("done");
  return top ? { state: top.attention, reason: top.reason, pane: top.id } : { state: "idle" };
}

/** What can be done about a pane's reason from a menu (M11: Rerun, #680:
 * Continue, and Dismiss), for someone who may. */
export function reasonItems(client: Client, pane: PaneId, reason: Reason | null | undefined): MenuItem[] {
  const tab = client.tabOfPane(pane);
  const session = tab ? client.sessionOfTab(tab.id) : undefined;
  if (!reason || client.role(session ?? null) === "viewer") return [];
  const items: MenuItem[] = [];
  if (reason.actions.includes("rerun")) items.push({ label: `Rerun ${reason.command ?? ""}`.trim(), run: () => void client.act({ action: "rerun", pane }) });
  if (reason.actions.includes("continue")) items.push({ label: "Continue", run: () => void client.act({ action: "continue", pane }) });
  if (reason.actions.includes("dismiss")) items.push({ label: "Dismiss", run: () => void client.act({ action: "dismiss", pane }) });
  return items;
}

/** A dot for "needs you", a tick for "done", a "!" for a failure (M24;
 * not a cross, which reads as "remove", UI audit #185); the reason's
 * headline is its title. With a client and pane, a failure that can be
 * run again is a menu button with Rerun (M11). */
export function AttentionBadge({ state, reason, pane, client }: { state: Attention; reason?: Reason | null; pane?: PaneId; client?: Client }) {
  if (state !== "needs_input" && state !== "done") return null;
  const failed = reason?.kind === "failed" || reason?.kind === "exited";
  const label = reason?.headline ?? (state === "done" ? "Finished" : "Needs you");
  const items = client && pane !== undefined && (reason?.actions.includes("rerun") || reason?.actions.includes("continue")) ? reasonItems(client, pane, reason) : [];
  const cls = `att ${state}${failed ? " failed" : ""}${items.length ? " acts" : ""}`;
  const mark = failed ? "!" : state === "done" ? "✓" : "●";
  if (!items.length) {
    return (
      <span class={cls} title={label} aria-label={label}>
        {mark}
      </span>
    );
  }
  return (
    <button
      type="button"
      class={cls}
      title={label}
      aria-label={label}
      aria-haspopup="menu"
      data-rerun-badge={pane}
      onPointerDown={(e) => e.stopPropagation()}
      // Its own keys: a tab's Enter and Space select the tab.
      onKeyDown={(e) => (e.key === "Enter" || e.key === " ") && e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        // Under the badge, from a key as from a click.
        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
        openMenu({ clientX: r.left, clientY: r.bottom + 4, preventDefault: () => e.preventDefault() }, [{ header: label }, ...items], e.currentTarget as HTMLElement);
      }}
    >
      {mark}
    </button>
  );
}
