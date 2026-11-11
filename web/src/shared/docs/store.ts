// A project's Docs (aru-118), as the platform's Doc routes give them to the
// reader (crates/platform/src/project_docs.rs): the list, one Doc with its
// sections, and the writes. A Doc is the project's, in the machine Doc's
// shape (a title, then sections of Markdown, pending until written), so the
// page draws it with the Doc block's renderer.

import type { NewProjectDoc, ProjectDoc, ProjectDocCopies, ProjectDocCopy, ProjectDocList, ProjectDocPublic, ProjectDocView, ProjectDocWrite } from "../proto";
import { chat } from "../chat-store";
import { platformFetch, refusalOf } from "../maintenance";

/** A refusal from the Doc routes, with its status and what else it said
 * (a 409 names the Doc that is there, or that the team edited it). */
export class DocError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly body: { id?: string; edited?: boolean } = {},
  ) {
    super(message);
  }
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await platformFetch(path, {
    method,
    headers: body === undefined ? {} : { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: "same-origin",
  });
  const j = (await res.json().catch(() => ({}))) as T & { error?: string; id?: string; edited?: boolean };
  if (!res.ok) throw new DocError(res.status, refusalOf(res.status, j, `HTTP ${res.status}`), { id: j.id, edited: j.edited });
  return j;
}

const enc = encodeURIComponent;
const docPath = (project: string, id: string) => `/api/projects/${enc(project)}/docs/${enc(id)}`;

/** The Docs' titles the page has read, for their links' slugs and tabs. */
const titles = new Map<string, string>();

/** A Doc as the page reads it: a platform before Arugula #678 sent no
 * `machine_name` (and one before #418 no `machine`), so a row from one
 * is a Doc made here, not a copy; and none says `made_from` before Share
 * with…. */
const doc = (d: ProjectDoc): ProjectDoc => ({ ...d, machine: d.machine ?? null, machine_name: d.machine_name ?? null, made_from: d.made_from ?? null });
const view = (v: ProjectDocView): ProjectDocView => ({ ...v, doc: doc(v.doc) });

export const projectDocs = {
  title: (id: string): string | undefined => titles.get(id),
  async list(project: string): Promise<ProjectDocList> {
    const l = await call<ProjectDocList>("GET", `/api/projects/${enc(project)}/docs`);
    for (const d of l.docs) titles.set(d.id, d.title);
    return { ...l, docs: l.docs.map(doc) };
  },
  async create(project: string, make: NewProjectDoc): Promise<ProjectDocView> {
    const v = await call<ProjectDocView>("POST", `/api/projects/${enc(project)}/docs`, make);
    titles.set(v.doc.id, v.doc.title);
    return view(v);
  },
  async get(project: string, id: string): Promise<ProjectDocView> {
    const v = await call<ProjectDocView>("GET", docPath(project, id));
    titles.set(id, v.doc.title);
    return view(v);
  },
  /** Rename it, write a section, or add one (what isn't given stays). */
  async write(project: string, id: string, w: Partial<ProjectDocWrite>): Promise<ProjectDocView> {
    const full: ProjectDocWrite = { title: null, section: null, heading: null, intent: null, markdown: null, ...w };
    const v = await call<ProjectDocView>("PUT", docPath(project, id), full);
    titles.set(id, v.doc.title);
    return view(v);
  },
  /** The team Docs made from one of its owner's Docs, a machine's copy or one written here. */
  copies: (project: string, id: string) => call<ProjectDocCopies>("GET", `${docPath(project, id)}/copies`),
  /** Share with…: a team Doc in `to` from the copy as it is now. */
  share: async (project: string, id: string, to: string): Promise<ProjectDocView> =>
    view(await call<ProjectDocView>("POST", `${docPath(project, id)}/copies`, { to })),
  /** Update copy: the team Doc in `to` takes the copy as it is now. A team
   * that edited it since makes this a 409 with `body.edited`, unless `replace`. */
  updateCopy: (project: string, id: string, to: string, replace: boolean) =>
    call<ProjectDocCopy>("PUT", `${docPath(project, id)}/copies/${enc(to)}`, { replace }),
  /** Arugula #871: make the Doc's public link (or read the one it has): the
   * platform's id and key, at version 0 until a client has put a version. */
  makePublic: (project: string, id: string) => call<ProjectDocPublic>("PUT", `${docPath(project, id)}/public`, {}),
  /** Say a version was committed at control, sealed from the Doc at `rev`. */
  servedPublic: (project: string, id: string, ver: number, rev: number) =>
    call<ProjectDocPublic>("POST", `${docPath(project, id)}/public`, { ver, rev }),
  /** Forget the link, once control has taken it down. */
  forgetPublic: (project: string, id: string) => call<Record<string, never>>("DELETE", `${docPath(project, id)}/public`),
  /** One of the Doc's pictures, the bytes. */
  async assetBytes(project: string, id: string, name: string): Promise<Uint8Array> {
    const res = await platformFetch(`${docPath(project, id)}/assets/${enc(name)}`, { credentials: "same-origin" });
    if (!res.ok) throw new DocError(res.status, `can't read the picture ${name}`);
    return new Uint8Array(await res.arrayBuffer());
  },
  remove: (project: string, id: string) => call<{ deleted: string }>("DELETE", docPath(project, id)),
  /** The Doc's thread (made the first time someone opens it): the room the
   * platform pushes a change on, so a page hears of it live. */
  thread: (id: string) => chat.openObject(id),
};
