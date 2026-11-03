// A project's "Not on a board" (#292, #656): its open issues that sit on no
// board, each with Add to board…, at the foot of the Boards page's open boards.
// `/p/:project/triage` is that page, scrolled here. The list is the platform's
// work route (crates/platform/src/boards.rs) asked for the untriaged; the
// issues list stays at All issues.

import { useEffect, useRef, useState } from "preact/hooks";
import type { BoardWorkItem, BoardWorkItems } from "../../shared/proto";
import { boards } from "../../shared/boards/store";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { openMenu, type MenuItem } from "../../shared/ui/menu";
import { names } from "../links";
import { follow, pathOf } from "../router";
import type { PlatformProject } from "../session";
import { toast } from "../toast";
import { mayWrite } from "./issues";
import { WaitingWrites } from "./tracker-writes";
import { TRIAGE_LEDE, TRIAGE_NONE, itemHref, sourceLabel, targets, without, type Target } from "./triage-text";

/** The project's open issues on no board: the newest first, at most 100. */
export async function untriaged(project: string): Promise<BoardWorkItem[]> {
  const res = await platformFetch(`/api/projects/${encodeURIComponent(project)}/work?untriaged=true&limit=100`, { credentials: "same-origin" });
  const j = (await res.json().catch(() => ({}))) as BoardWorkItems & { error?: string };
  if (!res.ok) throw new Error(refusalOf(res.status, j, `HTTP ${res.status}`));
  return j.items;
}

/** The open issues on no board, `null` while they load, for each of `projects` (read in parallel); one that can't be read counts none. */
export function useUntriaged(projects: readonly string[]): Map<string, number> | null {
  const [counts, setCounts] = useState<Map<string, number> | null>(null);
  const key = projects.join(",");
  useEffect(() => {
    setCounts(null);
    let live = true;
    void Promise.all(
      projects.map((id) =>
        untriaged(id).then(
          (l): [string, number] => [id, l.length],
          (): [string, number] => [id, 0],
        ),
      ),
    ).then((all) => live && setCounts(new Map(all)));
    return () => void (live = false);
  }, [key]);
  return counts;
}

/** A project's "Not on a board" on its Boards page: the open issues on no board, each with Add to board…. `added` is told when one went onto a board, so the boards read again; `scroll` brings the section into view once it has its list (`/triage`). */
export function NotOnBoard({ p, added, scroll }: { p: PlatformProject; added: () => void; scroll?: boolean }) {
  const [items, setItems] = useState<BoardWorkItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The boards, read the first time Add to board… is used.
  const where = useRef<Promise<ReturnType<typeof targets>> | null>(null);
  const top = useRef<HTMLElement>(null);
  const all = pathOf({ page: "issues", project: p.id }, names);
  useEffect(() => {
    let live = true;
    setError(null);
    untriaged(p.id).then(
      (l) => live && setItems(l),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [p.id]);
  const ready = items !== null || error !== null;
  useEffect(() => {
    if (scroll && ready) top.current?.scrollIntoView({ block: "start" });
  }, [scroll, ready]);

  const add = (i: BoardWorkItem, t: Target) =>
    boards.put(t.board, { entry: "row", pointer: i.pointer }).then(
      (v) => {
        setItems((l) => l && without(l, i.pointer));
        added();
        // A PR that goes with an issue's row is attached to it, not added.
        if (v.said) toast(v.said);
      },
      (e: Error) => toast(`Couldn't add it to the board: ${e.message}`),
    );
  const menu = async (e: MouseEvent, i: BoardWorkItem) => {
    const from = e.currentTarget as HTMLElement;
    const at = { clientX: e.clientX, clientY: e.clientY, preventDefault: () => e.preventDefault() };
    e.preventDefault();
    try {
      where.current ??= boards.list(p.id).then((l) => targets(l.boards));
      const open = await where.current;
      const entries: MenuItem[] = open.length
        ? open.map((b) => ({ label: b.title, run: () => void add(i, { board: b.board }) }))
        : [{ label: "No open board to add to", run: () => {}, disabled: true }];
      openMenu(at, entries, { from });
    } catch (err) {
      where.current = null;
      toast(`Couldn't read the boards: ${(err as Error).message}`);
    }
  };

  return (
    <section ref={top} id="triage" class="boards-untriaged" aria-labelledby={`untriaged-${p.id}`} data-boards-untriaged>
      <div class="sw-section-head">
        <h2 id={`untriaged-${p.id}`}>Not on a board{items && ` (${items.length})`}</h2>
        <a class="sw-all" href={all} data-triage-all onClick={(e) => follow(e, all)}>
          All issues ›
        </a>
      </div>
      <p class="dim">{TRIAGE_LEDE}</p>
      {mayWrite(p) && <WaitingWrites project={p.id} />}
      {error && <p class="control-error" role="alert">{error}</p>}
      {!error && items === null && <p class="dim">Loading…</p>}
      {items?.length === 0 && (
        <p class="dim" data-triage-none>
          {TRIAGE_NONE}
        </p>
      )}
      <ul class="platform-list triage-list" data-triage-list>
        {items?.map((i) => {
          const href = itemHref(i, (id) => pathOf({ page: "issue", project: p.id, issue: id }, names));
          const github = i.source === "github" || i.source === "plane";
          return (
            <li key={JSON.stringify(i.pointer)} class="triage-row" data-triage-row={sourceLabel(i)}>
              {href ? (
                <a class="triage-label" href={href} {...(github ? { target: "_blank", rel: "noopener noreferrer" } : { onClick: (e: MouseEvent) => follow(e, href) })}>
                  {sourceLabel(i)}
                </a>
              ) : (
                <span class="triage-label dim">{sourceLabel(i)}</span>
              )}
              <span class="triage-title">{i.title}</span>
              {i.assignees.length > 0 && <span class="triage-who dim">{i.assignees.join(", ")}</span>}
              {mayWrite(p) && (
                <button class="secondary triage-add" data-triage-add aria-haspopup="menu" onClick={(e) => void menu(e, i)}>
                  Add to board…
                </button>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
