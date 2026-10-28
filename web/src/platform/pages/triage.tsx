// `/p/:project/triage` (#292): with boards on, a project's Issues place is
// its open issues that sit on no board, each with Add to board…. The list is
// the platform's work route (crates/platform/src/boards.rs) asked for the
// untriaged; the issues list stays at All issues. Behind the `boards` Labs
// flag, as the boards are: the route answers 404 while it's off.

import { useEffect, useRef, useState } from "preact/hooks";
import type { BoardWorkItem, BoardWorkItems } from "../../shared/proto";
import { boards } from "../../shared/boards/store";
import { platformFetch, refusalOf } from "../../shared/maintenance";
import { useSubscribe } from "../../shared/ui/hooks";
import { openMenu, type MenuItem } from "../../shared/ui/menu";
import { PageHeader } from "../../shared/ui/page-header";
import { names } from "../links";
import { follow, pathOf } from "../router";
import { session, type PlatformProject } from "../session";
import { toast } from "../toast";
import { useBoards } from "./boards";
import { mayWrite } from "./issues";
import { Missing } from "./project";
import { TRIAGE_LEDE, TRIAGE_NONE, TRIAGE_OFF, itemHref, sourceLabel, targets, without, type Target } from "./triage-text";

/** The project's open issues on no board: the newest first, at most 100. */
async function untriaged(project: string): Promise<BoardWorkItem[]> {
  const res = await platformFetch(`/api/projects/${encodeURIComponent(project)}/work?untriaged=true&limit=100`, { credentials: "same-origin" });
  const j = (await res.json().catch(() => ({}))) as BoardWorkItems & { error?: string };
  if (!res.ok) throw new Error(refusalOf(res.status, j, `HTTP ${res.status}`));
  return j.items;
}

export function TriagePage({ project }: { project: string }) {
  useSubscribe((fn) => session.subscribe(fn));
  const p = session.project(project);
  if (!p) return <Missing what="That project isn't one of yours (or it's gone)." />;
  return <Triage p={p} />;
}

function Triage({ p }: { p: PlatformProject }) {
  const on = useBoards(p.id);
  const [items, setItems] = useState<BoardWorkItem[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  // The boards and their zones, read the first time Add to board… is used.
  const where = useRef<Promise<ReturnType<typeof targets>> | null>(null);
  const all = pathOf({ page: "issues", project: p.id }, names);
  const off = on === "off";
  useEffect(() => {
    if (on === null || off) return;
    let live = true;
    setError(null);
    untriaged(p.id).then(
      (l) => live && setItems(l),
      (e: Error) => live && setError(e.message),
    );
    return () => void (live = false);
  }, [p.id, on === null || off]);

  const add = (i: BoardWorkItem, t: Target) =>
    boards.put(t.board, { entry: "row", zone: t.zone, pointer: i.pointer }).then(
      () => setItems((l) => l && without(l, i.pointer)),
      (e: Error) => toast(`Couldn't add it to the board: ${e.message}`),
    );
  const menu = async (e: MouseEvent, i: BoardWorkItem) => {
    const from = e.currentTarget as HTMLElement;
    const at = { clientX: e.clientX, clientY: e.clientY, preventDefault: () => e.preventDefault() };
    e.preventDefault();
    try {
      where.current ??= boards.list(p.id).then((l) => Promise.all(l.boards.map((b) => boards.get(b.id)))).then(targets);
      const boardsAndZones = await where.current;
      const entries: MenuItem[] = boardsAndZones.length
        ? boardsAndZones.map((b) => ({
            label: b.title,
            submenu: b.zones.map((z) => ({ label: z.title, run: () => void add(i, { board: b.board, zone: z.id }) })),
          }))
        : [{ label: "No board has a zone to add to", run: () => {}, disabled: true }];
      openMenu(at, entries, { from });
    } catch (err) {
      where.current = null;
      toast(`Couldn't read the boards: ${(err as Error).message}`);
    }
  };

  return (
    <section class="platform-section" data-triage-page>
      <PageHeader
        title="Triage"
        sub={
          <>
            {TRIAGE_LEDE}{" "}
            <a href={all} data-triage-all onClick={(e) => follow(e, all)}>
              All issues
            </a>
          </>
        }
      />
      {off && (
        <p class="dim" data-triage-off>
          {TRIAGE_OFF}
        </p>
      )}
      {error && <p class="control-error" role="alert">{error}</p>}
      {!off && !error && items === null && <p class="dim">Loading…</p>}
      {items?.length === 0 && (
        <p class="dim" data-triage-none>
          {TRIAGE_NONE}
        </p>
      )}
      <ul class="platform-list triage-list" data-triage-list>
        {items?.map((i) => {
          const href = itemHref(i, (id) => pathOf({ page: "issue", project: p.id, issue: id }, names));
          const github = i.source === "github";
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
