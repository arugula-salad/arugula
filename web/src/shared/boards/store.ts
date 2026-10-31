// A project's boards (boards, step 2), as the platform's board
// routes give them to the reader (crates/platform/src/boards.rs): the list,
// one board with its zones and rows (and each `fi_` and `iss_` row's state
// now as `live`), and the writes. Shared: the platform page's board page
// and the shell's board block read through it. A write that names a stale
// `rev` is refused with 409 (`stale_rev`): the view reads the board again.

import type { BoardAttachRole, ForgeContent, NativeIssue, BoardList, BoardPointer, BoardPutEntry, BoardView, BoardWorkItems, RelationKind, TrackerConnections, TrackerWriteDone } from "../proto";
import { chat } from "../chat-store";
import { platformFetch, refusalOf } from "../maintenance";

/** A refusal from the board routes: its status, and `reason` when it gave
 * one (`stale_rev` for a write on an entry that moved on). */
export class BoardError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly reason?: string,
  ) {
    super(message);
  }
}

/** A PR's checks as `GET /api/forge/items/{id}/checks` gives them (control-wire's `ForgeChecks`, not generated yet). */
export interface ForgeChecks {
  item: string;
  head_sha: string | null;
  checks: { kind: string; name: string; status: string; conclusion: string | null; url: string | null; updated_at: number }[];
}

export const STALE_REV = "stale_rev";

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await platformFetch(path, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  const j = (await res.json().catch(() => ({}))) as T & { error?: string; reason?: string };
  if (!res.ok) throw new BoardError(res.status, refusalOf(res.status, j, `HTTP ${res.status}`), j.reason);
  return j;
}

const enc = encodeURIComponent;

/** The boards' titles the page has read, for their links' slugs. */
const titles = new Map<string, string>();

export const boards = {
  title: (id: string): string | undefined => titles.get(id),
  async list(project: string): Promise<BoardList> {
    const l = await call<BoardList>("GET", `/api/projects/${enc(project)}/boards`);
    for (const b of l.boards) titles.set(b.id, b.title);
    return l;
  },
  async create(project: string, title: string): Promise<BoardView> {
    const v = await call<BoardView>("POST", `/api/projects/${enc(project)}/boards`, { title });
    titles.set(v.board.id, v.board.title);
    return v;
  },
  async get(id: string): Promise<BoardView> {
    const v = await call<BoardView>("GET", `/api/boards/${enc(id)}`);
    titles.set(id, v.board.title);
    return v;
  },
  /** Add a row, or change one's note or pointer: what isn't given is left as it is. The answer's `said` tells when the platform attached a PR to the row of its issue instead of adding it, or found it there already. */
  put(id: string, p: Partial<BoardPutEntry>) {
    const full: BoardPutEntry = { id: null, rev: null, entry: null, zone: null, title: null, kind: null, pointer: null, note: null, status: null, ...p };
    return call<BoardView>("PUT", `/api/boards/${enc(id)}/entries`, full);
  },
  /** Attach work (a PR, an agent block, a run) or proof (a run, a link) to a row. */
  attach: (id: string, row: string, role: BoardAttachRole, pointer: BoardPointer) => call<BoardView>("POST", `/api/boards/${enc(id)}/attachments`, { row, role, pointer }),
  /** Answer an ask with one of its options, or with text when it has none: the person it names, or an owner or editor for them. */
  answer: (id: string, ask: string, a: { option: string } | { text: string }) =>
    call<BoardView>("POST", `/api/boards/${enc(id)}/asks/${enc(ask)}/answer`, { option: null, text: null, ...a }),
  /** Relate two items of the project (owners and editors): `from` `kind` `to`, ids that start iss_ or fi_ (#403). The board is read again after. */
  relate: (project: string, r: { from: string; to: string; kind: RelationKind }) => call<unknown>("POST", `/api/projects/${enc(project)}/relations`, r),
  /** Take a relation off; one that comes from Plane is refused. */
  unrelate: (project: string, relation: string) => call<unknown>("DELETE", `/api/projects/${enc(project)}/relations/${enc(relation)}`),
  detach: (id: string, attachment: string) => call<BoardView>("DELETE", `/api/boards/${enc(id)}/attachments/${enc(attachment)}`),
  /** Rename the board, or change its goal (empty clears it). */
  edit: (id: string, e: { title?: string; goal?: string }) => call<BoardView>("PATCH", `/api/boards/${enc(id)}`, e),
  /** Close the board as an epic, with what a person can now do. */
  close: (id: string, done_note: string) => call<BoardView>("POST", `/api/boards/${enc(id)}/close`, { done_note }),
  reopen: (id: string) => call<BoardView>("POST", `/api/boards/${enc(id)}/reopen`, {}),
  remove: (id: string, entry: string) => call<BoardView>("DELETE", `/api/boards/${enc(id)}/entries/${enc(entry)}`),
  /** Move a row to a group (`zone`, null: none), or a group among groups (`before`, null: last). */
  move: (id: string, entry: string, rev: number, m: { zone?: string | null; before?: string | null }) =>
    call<BoardView>("POST", `/api/boards/${enc(id)}/entries/${enc(entry)}/move`, { rev, before: null, ...m }),
  /** The drawer's reads, through the routes the Issues page and the forge blocks use: a native issue, a mirrored item's body and comments, a PR's checks. */
  issue: (id: string) => call<NativeIssue>("GET", `/api/issues/${enc(id)}`),
  content: (item: string) => call<ForgeContent>("GET", `/api/forge/items/${enc(item)}/content`),
  checks: (item: string) => call<ForgeChecks>("GET", `/api/forge/items/${enc(item)}/checks`),
  /** A project's work items matching `q`, newest first: what a row may point at. */
  work(project: string, q: string, limit: number): Promise<BoardWorkItems> {
    return call<BoardWorkItems>("GET", `/api/projects/${enc(project)}/work?q=${enc(q)}&limit=${limit}`);
  },
  /** The project's live Plane connections (#359), for a zone's New issue…: none when there are none, or boards are off. */
  async plane(project: string): Promise<{ id: string; identifier: string }[]> {
    try {
      const v = await call<TrackerConnections>("GET", `/api/projects/${enc(project)}/trackers`);
      return v.connections.filter((c) => c.purge_after === null).map((c) => ({ id: c.id, identifier: c.identifier }));
    } catch {
      return [];
    }
  },
  /** A person's new item in a Plane connection: lands at once; its `fi_` id is `item`. */
  planeCreate: (project: string, connection: string, title: string) =>
    call<TrackerWriteDone>("POST", `/api/projects/${enc(project)}/trackers/${enc(connection)}/writes`, { kind: "create", connection: null, title, body: null, state: null }),
  /** The board's thread (made the first time someone opens it): the room
   * the platform pushes a change on, so a view hears of it live. */
  thread: (id: string) => chat.openObject(id),
};
