// The board, arranged (Lift 5b, design lift §3.12; Arugula #1497): what the
// lead and you open beside the board's lanes, from the board's place-layout
// record (../place-layout.ts) and drawn with the kit's Tabs and panes
// (../kit/panes/). The board's default panes are its lane groups; opened
// panes are Docs, PRs, previews, runs, cards, terminals and prompts (a
// blocked agent's permission or question, answered in place). A review
// waits in a tab of its own, with a Needs-you row pointing at it. Nothing
// here switches you to a tab or moves the keys.

import type { ComponentChildren } from "preact";
import type { BoardRow, BoardView, InboxItem, LayoutPane, LayoutTab, PlaceLayout } from "../proto";
import { usePlatformApi } from "../platform-api";
import { HOME } from "../place-layout";
import { DocsIcon, IssueIcon, PaneTypeIcon, PrIcon, PulseIcon, ExternalIcon } from "../ui/icons";
import type { MenuItem } from "../ui/menu";
import type { PaneItem } from "../kit/panes/panes";
import type { OpenPr } from "./text";

/** The board's place: its layout record's key. */
export const boardPlace = (id: string) => `board:${id}`;

/** A board's default panes' names: each group's id (`zon_…`), and `none` for its rows in no group; `lanes` for a board with no groups. */
export const LANES = "lanes";

/** Who opened a pane or changed the layout, as its title row says it: the lead, or you. */
export const byWord = (by: { machine?: string | null }) => (by.machine ? "lead" : "you");

/** A pane's title row's words after its title: who opened it, and what it stands in place of. */
export function paneMeta(p: LayoutPane, defaultTitle: (name: string) => string): string {
  return [p.in_place_of ? `in place of ${defaultTitle(p.in_place_of)}` : null, byWord(p.opened_by), p.offline && p.machine ? `${p.machine} offline` : null].filter(Boolean).join(" · ");
}

export function paneIcon(kind: LayoutPane["kind"]): ComponentChildren {
  switch (kind) {
    case "doc":
      return <DocsIcon />;
    case "pr":
      return <PrIcon size="sm" />;
    case "card":
      return <IssueIcon size="sm" />;
    case "run":
      return <PulseIcon size="sm" />;
    case "preview":
      return <ExternalIcon size="sm" />;
    default:
      return <PaneTypeIcon type="terminal" />;
  }
}

/** The PR a pane's `ref` names (`owner/repo#N`, `#N` or its `fi_…`), among the board's open ones. */
export const prOf = (ref: string, prs: OpenPr[]) => prs.find((p) => p.id === ref || (p.live.number !== undefined && ref.endsWith(`#${p.live.number}`)));

/** The row a card pane's `ref` (`iss_…`) is. */
export const rowOf = (ref: string, view: BoardView): BoardRow | undefined => view.zones.flatMap((z) => z.rows).find((r) => "id" in r.pointer && r.pointer.id === ref);

/** What an opened pane shows, by its kind; the board's own (a Doc, a PR's or a card's drawer) come from `own`. */
export function PaneBody({ p, project, platform, own }: { p: LayoutPane; project: string; platform: (path: string) => string | undefined; own: (p: LayoutPane) => ComponentChildren | null }) {
  const api = usePlatformApi();
  const mine = own(p);
  if (mine) return <>{mine}</>;
  const pane = p.ref.replace(/^%/, "");
  // Its machine is away: the state it was last shown with.
  if (p.offline || ((p.kind === "terminal" || p.kind === "prompt") && !p.machine))
    return (
      <p class="m-meta board-pane-note" data-pane-offline>
        {p.state || "Nothing to show"} · {p.machine ? `${p.machine} is offline` : "no machine"}
      </p>
    );
  if (p.kind === "prompt" && api.LiveAsk) return <api.LiveAsk machine={p.machine!} pane={pane} />;
  if (p.kind === "terminal" && api.LiveWork) return <api.LiveWork machine={p.machine!} pane={pane} />;
  const href = p.kind === "run" ? platform(`/p/${project}/runs/${p.ref}`) : p.kind === "card" ? platform(`/p/${project}/issues/${p.ref}`) : p.kind === "doc" ? platform(`/p/${project}/docs/${p.ref}`) : /^https?:\/\//.test(p.ref) ? p.ref : undefined;
  return (
    <div class="board-pane-note">
      {p.state && <p class="m-meta">{p.state}</p>}
      {href ? (
        <a class="k-button is-plain" href={href} target={href.startsWith("http") ? "_blank" : undefined} rel="noreferrer" data-pane-open>
          Open {p.title}
        </a>
      ) : (
        <p class="m-meta">{p.ref}</p>
      )}
    </div>
  );
}

/** "2:24 PM". */
const timeOfDay = (at: number) => new Date(at).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });

/** "changed by lead · 2:24 PM · Reset", while the home tab differs from the board's default. */
export function ChangedLine({ layout, reset }: { layout: PlaceLayout; reset: () => void }) {
  const c = layout.changed;
  if (!c) return null;
  return (
    <span class="m-meta board-changed" data-board-changed={byWord(c.by)}>
      changed by {byWord(c.by)} · {timeOfDay(c.at)} ·{" "}
      <button type="button" class="link" data-board-reset onClick={reset}>
        Reset
      </button>
    </span>
  );
}

/** The ⋯'s layout items: Reset while changed, and the closed list to reopen from. */
export function layoutItems(layout: PlaceLayout, reset: () => void, reopen: (pane: string) => void): MenuItem[] {
  const closed = layout.closed.slice(0, 8);
  return [
    ...(layout.changed ? [{ label: "Reset the layout", run: reset, attrs: { "data-board-reset-menu": "" } }] : []),
    ...(closed.length ? ["separator" as const, ...closed.map((p) => ({ label: `Reopen ${p.title}`, run: () => reopen(p.id), attrs: { "data-board-reopen": p.id } }))] : []),
  ];
}

/** A tab's title: the board's for the home tab. */
export const tabTitle = (t: LayoutTab, board: string) => (t.id === HOME ? board : t.title || t.panes[0]?.title || "Tab");

/** The board's `for_you` items: what an agent put in a tab for you, waiting, by its tab. */
export function forYou(items: InboxItem[], place: string): InboxItem[] {
  return items.filter((i) => i.subject.place === place && i.subject.tab && !i.done_at);
}

/** A pane as the kit draws it, from the record. */
export function paneItem(p: LayoutPane, o: { body: ComponentChildren; meta: string; dot: boolean; arriving: boolean; mine: boolean; move?: () => void; max: () => void; close: () => void; restore?: { name: string; run: () => void } }): PaneItem {
  return {
    id: p.id,
    title: p.title,
    icon: paneIcon(p.kind),
    meta: o.meta,
    state: p.kind === "prompt" && !p.offline ? { state: "asks", word: "asks you" } : undefined,
    dot: o.dot,
    arriving: o.arriving,
    dim: p.offline,
    size: p.size ?? undefined,
    move: o.move,
    max: o.max,
    close: o.close,
    extra: o.restore ? (
      <button type="button" class="link m-meta" data-pane-restore={p.in_place_of ?? ""} onClick={o.restore.run}>
        ↺ {o.restore.name}
      </button>
    ) : undefined,
    body: o.body,
    data: { "data-layout-pane": p.id, "data-pane-kind": p.kind, "data-pane-by": byWord(p.opened_by), "data-pane-touched": p.touched ? "" : undefined },
  };
}
