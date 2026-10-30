// A project's boards (boards, step 2), as the platform's board
// routes give them to the reader (crates/platform/src/boards.rs): the list,
// one board with its zones and rows (and each `fi_` and `iss_` row's state
// now as `live`), and the writes. Shared: the platform page's board page
// and the shell's board block read through it. A write that names a stale
// `rev` is refused with 409 (`stale_rev`): the view reads the board again.

import type { BoardAttachRole, BoardList, BoardPointer, BoardPutEntry, BoardView, BoardWorkItems } from "../proto";
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
  detach: (id: string, attachment: string) => call<BoardView>("DELETE", `/api/boards/${enc(id)}/attachments/${enc(attachment)}`),
  /** Rename the board, or change its goal (empty clears it). */
  edit: (id: string, e: { title?: string; goal?: string }) => call<BoardView>("PATCH", `/api/boards/${enc(id)}`, e),
  /** Close the board as an epic, with what a person can now do. */
  close: (id: string, done_note: string) => call<BoardView>("POST", `/api/boards/${enc(id)}/close`, { done_note }),
  reopen: (id: string) => call<BoardView>("POST", `/api/boards/${enc(id)}/reopen`, {}),
  remove: (id: string, entry: string) => call<BoardView>("DELETE", `/api/boards/${enc(id)}/entries/${enc(entry)}`),
  /** A project's work items matching `q`, newest first: what a row may point at. */
  work(project: string, q: string, limit: number): Promise<BoardWorkItems> {
    return call<BoardWorkItems>("GET", `/api/projects/${enc(project)}/work?q=${enc(q)}&limit=${limit}`);
  },
  /** The board's thread (made the first time someone opens it): the room
   * the platform pushes a change on, so a view hears of it live. */
  thread: (id: string) => chat.openObject(id),
};
